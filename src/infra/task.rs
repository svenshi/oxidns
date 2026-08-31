// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Global scheduled task center.
//!
//! Unlike per-task ticker loops, this center uses one scheduler task to drive
//! fixed-interval maintenance and optional calendar-based jobs. This reduces
//! long-lived task overhead and keeps execution and lifecycle control
//! centralized in runtime.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::Duration;

use futures::stream::{FuturesUnordered, StreamExt};
use jiff::Timestamp;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio::time::{Instant, sleep_until};
use tracing::{error, warn};

use crate::infra::clock::AppClock;
use crate::infra::error::{DnsError, Result as DnsResult};

/// Run CPU- or blocking-I/O-heavy construction away from the async runtime.
///
/// Snapshot owners intentionally perform publication themselves after this
/// helper succeeds, so cancellation, panic, and build failure all leave the
/// currently published state untouched.
///
/// Dropping this future does not stop an already-running blocking closure.
/// Repeatable callers must therefore run it under a lifecycle task or permit
/// that remains owned until the closure exits.
#[allow(dead_code)]
pub(crate) async fn spawn_blocking_result<T, F>(task_name: &str, task: F) -> DnsResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> DnsResult<T> + Send + 'static,
{
    tokio::task::spawn_blocking(task)
        .await
        .map_err(|error| DnsError::runtime(format!("{task_name} blocking task failed: {error}")))?
}

/// Build a large snapshot on a one-shot OS thread.
///
/// Tokio's blocking worker only owns thread creation and joining. Matcher
/// construction happens on the child thread, so allocator thread-local caches
/// are released when that thread exits. Callers still publish snapshots only
/// after this helper succeeds.
///
/// Dropping this future cannot cancel the OS thread. Provider reloads and
/// runtime initialization keep their serialization ownership in detached
/// lifecycle tasks until this function returns.
pub(crate) async fn spawn_isolated_build<T, F>(task_name: &str, task: F) -> DnsResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> DnsResult<T> + Send + 'static,
{
    let thread_name = task_name.to_owned();
    tokio::task::spawn_blocking(move || {
        std::thread::Builder::new()
            .name(thread_name.clone())
            .spawn(task)
            .map_err(|error| {
                DnsError::runtime(format!(
                    "failed to start isolated build thread '{thread_name}': {error}"
                ))
            })?
            .join()
            .map_err(|panic| {
                let detail = panic
                    .downcast_ref::<&str>()
                    .copied()
                    .or_else(|| panic.downcast_ref::<String>().map(String::as_str))
                    .unwrap_or("unknown panic payload");
                DnsError::runtime(format!(
                    "isolated build thread '{thread_name}' panicked: {detail}"
                ))
            })?
    })
    .await
    .map_err(|error| {
        DnsError::runtime(format!(
            "{task_name} blocking thread coordinator failed: {error}"
        ))
    })?
}

const MANUAL_TRIGGER_CAPACITY: usize = 16;
const CRON_WALL_CLOCK_RECHECK_INTERVAL: Duration = Duration::from_secs(30);

type TaskFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;
type TaskFn = Arc<dyn Fn(TaskRunContext) -> TaskFuture + Send + Sync + 'static>;
type WallClock = Arc<dyn Fn() -> i64 + Send + Sync + 'static>;
pub(crate) type TaskEventHandler = Arc<dyn Fn(TaskEvent) + Send + Sync + 'static>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TaskTriggerKind {
    Scheduled,
    Manual,
}

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub(crate) struct TaskRunContext {
    pub trigger: TaskTriggerKind,
    pub scheduled_at_unix_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TaskEvent {
    Skipped { trigger: TaskTriggerKind },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TriggerOutcome {
    Started,
    AlreadyRunning,
    Unavailable,
}

#[derive(Clone, Default)]
pub(crate) struct TaskOptions {
    on_event: Option<TaskEventHandler>,
}

impl TaskOptions {
    #[allow(dead_code)] // Used by task owners that opt into scheduler events.
    pub(crate) fn on_event(mut self, handler: TaskEventHandler) -> Self {
        self.on_event = Some(handler);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TaskDeadline {
    instant: Instant,
    unix_ms: i64,
}

#[derive(Clone)]
struct TaskClock {
    base_instant: Instant,
    base_unix_ms: i64,
    wall_clock: WallClock,
}

impl TaskClock {
    fn new(wall_clock: WallClock) -> Self {
        Self {
            base_instant: Instant::now(),
            base_unix_ms: AppClock::now_timestamp().min(i64::MAX as u64) as i64,
            wall_clock,
        }
    }

    fn monotonic_now(&self) -> TaskDeadline {
        let instant = Instant::now();
        let elapsed_ms = instant
            .duration_since(self.base_instant)
            .as_millis()
            .min(i64::MAX as u128) as i64;
        TaskDeadline {
            instant,
            unix_ms: self.base_unix_ms.saturating_add(elapsed_ms),
        }
    }

    fn wall_now(&self) -> TaskDeadline {
        TaskDeadline {
            instant: Instant::now(),
            unix_ms: (self.wall_clock)(),
        }
    }

    fn now_for(&self, schedule: &TaskSchedule) -> TaskDeadline {
        if schedule.is_cron() {
            self.wall_now()
        } else {
            self.monotonic_now()
        }
    }
}

impl std::fmt::Debug for TaskClock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TaskClock")
            .field("base_instant", &self.base_instant)
            .field("base_unix_ms", &self.base_unix_ms)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone)]
enum TaskSchedule {
    FixedInterval {
        interval: Duration,
    },
    #[cfg(feature = "_task-cron")]
    Cron {
        expression: String,
        timezone: String,
        crontab: Arc<cronexpr::Crontab>,
    },
}

impl TaskSchedule {
    fn fixed(interval: Duration) -> Self {
        Self::FixedInterval { interval }
    }

    #[cfg(feature = "_task-cron")]
    fn cron(expression: &str, timezone: &str) -> DnsResult<Self> {
        let expression = expression.trim();
        let timezone = timezone.trim();
        if expression.split_whitespace().count() != 5 {
            return Err(DnsError::runtime(
                "cron schedule must use standard 5-field format; second-level cron is not supported",
            ));
        }
        if timezone.is_empty() {
            return Err(DnsError::runtime("cron schedule timezone cannot be empty"));
        }

        let schedule_with_timezone = format!("{expression} {timezone}");
        let crontab = cronexpr::parse_crontab(&schedule_with_timezone).map_err(|error| {
            DnsError::runtime(format!(
                "failed to parse cron schedule '{schedule_with_timezone}': {error}"
            ))
        })?;
        Ok(Self::Cron {
            expression: expression.to_string(),
            timezone: timezone.to_string(),
            crontab: Arc::new(crontab),
        })
    }

    fn next_after(
        &self,
        previous: TaskDeadline,
        now: TaskDeadline,
    ) -> DnsResult<Option<TaskDeadline>> {
        match self {
            Self::FixedInterval { interval } => {
                if interval.is_zero() {
                    return Err(DnsError::runtime(
                        "fixed task interval must be greater than zero",
                    ));
                }
                let interval_ns = interval.as_nanos();
                let elapsed_ns = now.instant.duration_since(previous.instant).as_nanos();
                let steps = elapsed_ns / interval_ns + 1;
                let remainder_ns = elapsed_ns % interval_ns;
                let delay_ns = interval_ns - remainder_ns;
                let delay = duration_from_nanos(delay_ns)?;
                let instant = now.instant.checked_add(delay).ok_or_else(|| {
                    DnsError::runtime("fixed task interval produced an invalid deadline")
                })?;
                let advance_ms = interval
                    .as_millis()
                    .saturating_mul(steps)
                    .min(i64::MAX as u128) as i64;
                Ok(Some(TaskDeadline {
                    instant,
                    unix_ms: previous.unix_ms.saturating_add(advance_ms),
                }))
            }
            #[cfg(feature = "_task-cron")]
            Self::Cron {
                expression,
                timezone,
                crontab,
            } => {
                let timestamp = Timestamp::from_millisecond(now.unix_ms).map_err(|error| {
                    DnsError::runtime(format!(
                        "failed to build timestamp '{}' for cron schedule '{} {}': {error}",
                        now.unix_ms, expression, timezone
                    ))
                })?;
                let next = crontab.find_next(timestamp).map_err(|error| {
                    DnsError::runtime(format!(
                        "failed to compute next run for cron schedule '{} {}': {error}",
                        expression, timezone
                    ))
                })?;
                let unix_ms = next.timestamp().as_millisecond();
                let delta_ms = unix_ms.checked_sub(now.unix_ms).ok_or_else(|| {
                    DnsError::runtime(format!(
                        "cron schedule '{} {}' produced an invalid deadline",
                        expression, timezone
                    ))
                })?;
                if delta_ms <= 0 {
                    return Err(DnsError::runtime(format!(
                        "cron schedule '{} {}' produced a non-future deadline",
                        expression, timezone
                    )));
                }
                Ok(Some(TaskDeadline {
                    instant: now.instant + Duration::from_millis(delta_ms as u64),
                    unix_ms,
                }))
            }
        }
    }

    fn is_cron(&self) -> bool {
        #[cfg(feature = "_task-cron")]
        {
            matches!(self, Self::Cron { .. })
        }
        #[cfg(not(feature = "_task-cron"))]
        {
            let _ = self;
            false
        }
    }

    fn reanchor_cron_deadline(
        &self,
        deadline: TaskDeadline,
        now: TaskDeadline,
    ) -> DnsResult<TaskDeadline> {
        #[cfg(feature = "_task-cron")]
        {
            let Self::Cron {
                expression,
                timezone,
                ..
            } = self
            else {
                return Ok(deadline);
            };
            let delta_ms = deadline.unix_ms.checked_sub(now.unix_ms).ok_or_else(|| {
                DnsError::runtime(format!(
                    "cron schedule '{expression} {timezone}' produced an invalid wall-clock delta"
                ))
            })?;
            let instant = if delta_ms <= 0 {
                now.instant
            } else {
                now.instant
                    .checked_add(Duration::from_millis(delta_ms as u64))
                    .ok_or_else(|| {
                        DnsError::runtime(format!(
                            "cron schedule '{expression} {timezone}' produced an invalid deadline"
                        ))
                    })?
            };
            Ok(TaskDeadline {
                instant,
                unix_ms: deadline.unix_ms,
            })
        }
        #[cfg(not(feature = "_task-cron"))]
        {
            let _ = (self, now);
            Ok(deadline)
        }
    }
}

fn duration_from_nanos(nanos: u128) -> DnsResult<Duration> {
    const NANOS_PER_SECOND: u128 = 1_000_000_000;
    let seconds = nanos / NANOS_PER_SECOND;
    if seconds > u64::MAX as u128 {
        return Err(DnsError::runtime(
            "fixed task interval produced an invalid deadline",
        ));
    }
    Ok(Duration::new(
        seconds as u64,
        (nanos % NANOS_PER_SECOND) as u32,
    ))
}

enum Command {
    Register {
        id: u64,
        name: String,
        clock: TaskClock,
        schedule: TaskSchedule,
        next_run: Option<TaskDeadline>,
        task: TaskFn,
        on_event: Option<TaskEventHandler>,
    },
    Remove {
        id: u64,
        ack: oneshot::Sender<()>,
    },
    RemoveDetached {
        id: u64,
    },
    StopAll {
        ack: oneshot::Sender<()>,
    },
}

struct TriggerCommand {
    id: u64,
    reply: oneshot::Sender<TriggerOutcome>,
}

struct ScheduledTask {
    name: String,
    clock: TaskClock,
    schedule: TaskSchedule,
    next_run: Option<TaskDeadline>,
    task: TaskFn,
    on_event: Option<TaskEventHandler>,
    running: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for ScheduledTask {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScheduledTask")
            .field("name", &self.name)
            .field("schedule", &self.schedule)
            .field("next_run", &self.next_run)
            .finish_non_exhaustive()
    }
}

/// Runtime-wide center for periodic background tasks.
pub struct TaskCenter {
    next_id: AtomicU64,
    channels: StdMutex<TaskCenterChannels>,
    wall_clock: WallClock,
}

#[derive(Debug, Clone)]
struct TaskCenterChannels {
    control_tx: mpsc::UnboundedSender<Command>,
    trigger_tx: mpsc::Sender<TriggerCommand>,
}

impl TaskCenter {
    fn new() -> Self {
        Self::with_wall_clock(Arc::new(|| Timestamp::now().as_millisecond()))
    }

    fn with_wall_clock(wall_clock: WallClock) -> Self {
        AppClock::start();
        Self {
            next_id: AtomicU64::new(1),
            channels: StdMutex::new(Self::spawn_scheduler()),
            wall_clock,
        }
    }

    fn spawn_scheduler() -> TaskCenterChannels {
        let (control_tx, control_rx) = mpsc::unbounded_channel();
        let (trigger_tx, trigger_rx) = mpsc::channel(MANUAL_TRIGGER_CAPACITY);
        tokio::spawn(run_scheduler(control_rx, trigger_rx));
        TaskCenterChannels {
            control_tx,
            trigger_tx,
        }
    }

    fn channels(&self) -> TaskCenterChannels {
        let mut channels = self
            .channels
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if channels.control_tx.is_closed() || channels.trigger_tx.is_closed() {
            *channels = Self::spawn_scheduler();
        }
        channels.clone()
    }

    fn spawn_fixed<F, Fut>(
        &self,
        name: impl Into<String>,
        interval: Duration,
        options: TaskOptions,
        task: F,
    ) -> DnsResult<ManagedTaskHandle>
    where
        F: Fn(TaskRunContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        if interval.is_zero() {
            return Err(DnsError::runtime(
                "fixed task interval must be greater than zero",
            ));
        }
        self.spawn_inner(name, TaskSchedule::fixed(interval), options, task)
    }

    #[cfg(feature = "_task-cron")]
    fn spawn_cron<F, Fut>(
        &self,
        name: impl Into<String>,
        expression: &str,
        timezone: &str,
        options: TaskOptions,
        task: F,
    ) -> DnsResult<ManagedTaskHandle>
    where
        F: Fn(TaskRunContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let schedule = TaskSchedule::cron(expression, timezone)?;
        self.spawn_inner(name, schedule, options, task)
    }

    fn spawn_inner<F, Fut>(
        &self,
        name: impl Into<String>,
        schedule: TaskSchedule,
        options: TaskOptions,
        task: F,
    ) -> DnsResult<ManagedTaskHandle>
    where
        F: Fn(TaskRunContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let name = name.into();
        let clock = TaskClock::new(self.wall_clock.clone());
        let now = clock.now_for(&schedule);
        let next_run = compute_next_deadline(&name, &schedule, now, now)?;
        let task: TaskFn = Arc::new(move |context| Box::pin(task(context)));
        let channels = self.channels();
        let command = Command::Register {
            id,
            name,
            clock,
            schedule,
            next_run,
            task,
            on_event: options.on_event,
        };

        let channels = self.send_registration(channels, command)?;

        Ok(ManagedTaskHandle {
            id,
            control_tx: channels.control_tx,
            trigger_tx: channels.trigger_tx,
        })
    }

    fn send_registration(
        &self,
        channels: TaskCenterChannels,
        command: Command,
    ) -> DnsResult<TaskCenterChannels> {
        let channels = match channels.control_tx.send(command) {
            Ok(()) => channels,
            Err(error) => {
                let retry_channels = self.channels();
                retry_channels
                    .control_tx
                    .send(error.0)
                    .map_err(|_| DnsError::runtime("task center is not running"))?;
                retry_channels
            }
        };
        Ok(channels)
    }

    /// Stop all managed tasks.
    pub async fn stop_all(&self) {
        let (ack_tx, ack_rx) = oneshot::channel();
        if self
            .channels()
            .control_tx
            .send(Command::StopAll { ack: ack_tx })
            .is_err()
        {
            return;
        }
        let _ = ack_rx.await;
    }
}

impl std::fmt::Debug for TaskCenter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TaskCenter")
            .field("next_id", &self.next_id)
            .field("channels", &self.channels)
            .finish_non_exhaustive()
    }
}

#[derive(Clone)]
#[allow(dead_code)] // Manual triggering is available even when no bundled caller exposes it.
pub(crate) struct ManagedTaskHandle {
    id: u64,
    control_tx: mpsc::UnboundedSender<Command>,
    trigger_tx: mpsc::Sender<TriggerCommand>,
}

impl std::fmt::Debug for ManagedTaskHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ManagedTaskHandle")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl ManagedTaskHandle {
    #[allow(dead_code)]
    pub(crate) async fn trigger(&self) -> TriggerOutcome {
        let (reply, response) = oneshot::channel();
        if self
            .trigger_tx
            .try_send(TriggerCommand { id: self.id, reply })
            .is_err()
        {
            return TriggerOutcome::Unavailable;
        }
        response.await.unwrap_or(TriggerOutcome::Unavailable)
    }

    pub(crate) async fn stop(&self) {
        let (ack_tx, ack_rx) = oneshot::channel();
        if self
            .control_tx
            .send(Command::Remove {
                id: self.id,
                ack: ack_tx,
            })
            .is_err()
        {
            return;
        }
        let _ = ack_rx.await;
    }

    pub(crate) fn stop_detached(&self) {
        let _ = self
            .control_tx
            .send(Command::RemoveDetached { id: self.id });
    }
}

#[hotpath::measure]
async fn run_scheduler(
    mut control_rx: mpsc::UnboundedReceiver<Command>,
    mut trigger_rx: mpsc::Receiver<TriggerCommand>,
) {
    let mut tasks: HashMap<u64, ScheduledTask> = HashMap::new();
    let mut deadlines: BinaryHeap<Reverse<(Instant, u64)>> = BinaryHeap::new();
    let mut cron_recheck_at = None;

    loop {
        let has_active_cron = tasks
            .values()
            .any(|task| task.schedule.is_cron() && task.next_run.is_some());
        if has_active_cron {
            cron_recheck_at
                .get_or_insert_with(|| Instant::now() + CRON_WALL_CLOCK_RECHECK_INTERVAL);
        } else {
            cron_recheck_at = None;
        }

        let next_deadline = next_deadline(&tasks, &mut deadlines);
        let next_wakeup = match (next_deadline, cron_recheck_at) {
            (Some(deadline), Some(recheck)) => Some(deadline.min(recheck)),
            (Some(deadline), None) => Some(deadline),
            (None, Some(recheck)) => Some(recheck),
            (None, None) => None,
        };
        if let Some(next_wakeup) = next_wakeup {
            tokio::select! {
                biased;
                command = control_rx.recv() => {
                    let Some(command) = command else {
                        break;
                    };
                    handle_command(command, &mut tasks, &mut deadlines);
                }
                _ = sleep_until(next_wakeup) => {
                    if cron_recheck_at.is_some() {
                        refresh_cron_deadlines(&mut tasks, &mut deadlines);
                    }
                    run_due_tasks(&mut tasks, &mut deadlines).await;
                    if cron_recheck_at.is_some() {
                        cron_recheck_at = Some(
                            Instant::now() + CRON_WALL_CLOCK_RECHECK_INTERVAL
                        );
                    }
                }
                trigger = trigger_rx.recv() => {
                    if let Some(trigger) = trigger {
                        handle_trigger(trigger, &mut tasks).await;
                    }
                }
            }
        } else {
            tokio::select! {
                biased;
                command = control_rx.recv() => {
                    let Some(command) = command else {
                        break;
                    };
                    handle_command(command, &mut tasks, &mut deadlines);
                }
                trigger = trigger_rx.recv() => {
                    if let Some(trigger) = trigger {
                        handle_trigger(trigger, &mut tasks).await;
                    }
                }
            }
        }
    }

    let mut running = Vec::new();
    for (_, mut task) in tasks.drain() {
        if let Some(handle) = task.running.take() {
            running.push((task.name, handle));
        }
    }
    stop_handles(running).await;
}

fn handle_command(
    cmd: Command,
    tasks: &mut HashMap<u64, ScheduledTask>,
    deadlines: &mut BinaryHeap<Reverse<(Instant, u64)>>,
) {
    match cmd {
        Command::Register {
            id,
            name,
            clock,
            schedule,
            next_run,
            task,
            on_event,
        } => {
            if let Some(next_run) = next_run {
                deadlines.push(Reverse((next_run.instant, id)));
            }
            tasks.insert(
                id,
                ScheduledTask {
                    name,
                    clock,
                    schedule,
                    next_run,
                    task,
                    on_event,
                    running: None,
                },
            );
        }
        Command::Remove { id, ack } => {
            if let Some(mut task) = tasks.remove(&id) {
                if let Some(handle) = task.running.take() {
                    stop_handle_with_ack(task.name, handle, ack);
                } else {
                    let _ = ack.send(());
                }
            } else {
                let _ = ack.send(());
            }
            rebuild_deadline_heap(tasks, deadlines);
        }
        Command::RemoveDetached { id } => {
            if let Some(mut task) = tasks.remove(&id)
                && let Some(handle) = task.running.take()
            {
                stop_handle_detached(task.name, handle);
            }
            rebuild_deadline_heap(tasks, deadlines);
        }
        Command::StopAll { ack } => {
            let mut running = Vec::new();
            for (_, mut task) in tasks.drain() {
                if let Some(handle) = task.running.take() {
                    running.push((task.name, handle));
                }
            }
            deadlines.clear();
            stop_handles_with_ack(running, ack);
        }
    }
}

fn refresh_cron_deadlines(
    tasks: &mut HashMap<u64, ScheduledTask>,
    deadlines: &mut BinaryHeap<Reverse<(Instant, u64)>>,
) {
    for task in tasks.values_mut() {
        if !task.schedule.is_cron() {
            continue;
        }
        let Some(deadline) = task.next_run else {
            continue;
        };
        let now = task.clock.wall_now();
        match task.schedule.reanchor_cron_deadline(deadline, now) {
            Ok(deadline) => {
                task.next_run = Some(deadline);
            }
            Err(error) => {
                task.next_run = None;
                error!(
                    task = %task.name,
                    error = %error,
                    "Failed to re-anchor cron task; automatic execution is paused"
                );
            }
        }
    }

    rebuild_deadline_heap(tasks, deadlines);
}

fn rebuild_deadline_heap(
    tasks: &HashMap<u64, ScheduledTask>,
    deadlines: &mut BinaryHeap<Reverse<(Instant, u64)>>,
) {
    deadlines.clear();
    for (&id, task) in tasks.iter() {
        if let Some(deadline) = task.next_run {
            deadlines.push(Reverse((deadline.instant, id)));
        }
    }
}

#[hotpath::measure]
async fn run_due_tasks(
    tasks: &mut HashMap<u64, ScheduledTask>,
    deadlines: &mut BinaryHeap<Reverse<(Instant, u64)>>,
) {
    let now = Instant::now();
    let mut finished = Vec::new();
    let mut to_spawn = Vec::new();

    while let Some(deadline) = next_deadline(tasks, deadlines) {
        if deadline > now {
            break;
        }
        let Some(Reverse((_, id))) = deadlines.pop() else {
            break;
        };

        let mut spawn_entry: Option<(u64, TaskFn, TaskRunContext)> = None;
        if let Some(task) = tasks.get_mut(&id) {
            let Some(scheduled_at) = task.next_run.take() else {
                continue;
            };
            let task_now = task.clock.now_for(&task.schedule);
            task.next_run =
                match compute_next_deadline(&task.name, &task.schedule, scheduled_at, task_now) {
                    Ok(next_run) => next_run,
                    Err(error) => {
                        error!(
                            task = %task.name,
                            error = %error,
                            "Failed to advance scheduled task; automatic execution is paused"
                        );
                        None
                    }
                };
            if let Some(next_run) = task.next_run {
                deadlines.push(Reverse((next_run.instant, id)));
            }

            if task
                .running
                .as_ref()
                .is_some_and(|handle| handle.is_finished())
                && let Some(handle) = task.running.take()
            {
                finished.push((task.name.clone(), handle));
            }

            if task.running.is_none() {
                spawn_entry = Some((
                    id,
                    task.task.clone(),
                    TaskRunContext {
                        trigger: TaskTriggerKind::Scheduled,
                        scheduled_at_unix_ms: scheduled_at.unix_ms,
                    },
                ));
            } else {
                notify_task_event(
                    &task.name,
                    task.on_event.as_ref(),
                    TaskEvent::Skipped {
                        trigger: TaskTriggerKind::Scheduled,
                    },
                );
            }
        }

        if let Some(spawn_entry) = spawn_entry {
            to_spawn.push(spawn_entry);
        }
    }

    reap_finished_handles(finished).await;
    for (id, run, context) in to_spawn {
        if let Some(task) = tasks.get_mut(&id) {
            task.running = Some(tokio::spawn(async move {
                run(context).await;
            }));
        }
    }
}

async fn handle_trigger(trigger: TriggerCommand, tasks: &mut HashMap<u64, ScheduledTask>) {
    let Some(task) = tasks.get_mut(&trigger.id) else {
        let _ = trigger.reply.send(TriggerOutcome::Unavailable);
        return;
    };

    if task
        .running
        .as_ref()
        .is_some_and(|handle| handle.is_finished())
        && let Some(handle) = task.running.take()
    {
        await_finished_handle(task.name.clone(), handle).await;
    }

    if task.running.is_some() {
        notify_task_event(
            &task.name,
            task.on_event.as_ref(),
            TaskEvent::Skipped {
                trigger: TaskTriggerKind::Manual,
            },
        );
        let _ = trigger.reply.send(TriggerOutcome::AlreadyRunning);
        return;
    }

    let now = task.clock.now_for(&task.schedule);
    let run = task.task.clone();
    task.running = Some(tokio::spawn(async move {
        run(TaskRunContext {
            trigger: TaskTriggerKind::Manual,
            scheduled_at_unix_ms: now.unix_ms,
        })
        .await;
    }));
    let _ = trigger.reply.send(TriggerOutcome::Started);
}

fn compute_next_deadline(
    task_name: &str,
    schedule: &TaskSchedule,
    previous: TaskDeadline,
    now: TaskDeadline,
) -> DnsResult<Option<TaskDeadline>> {
    let result =
        catch_unwind(AssertUnwindSafe(|| schedule.next_after(previous, now))).map_err(|_| {
            DnsError::runtime(format!(
                "schedule calculation panicked for task '{task_name}'"
            ))
        })??;
    if let Some(deadline) = result
        && deadline.instant <= now.instant
    {
        return Err(DnsError::runtime(format!(
            "schedule for task '{task_name}' produced a non-future deadline"
        )));
    }
    Ok(result)
}

fn notify_task_event(task_name: &str, handler: Option<&TaskEventHandler>, event: TaskEvent) {
    let Some(handler) = handler else {
        return;
    };
    if catch_unwind(AssertUnwindSafe(|| handler(event))).is_err() {
        error!(task = %task_name, "Scheduled task event handler panicked");
    }
}

fn next_deadline(
    tasks: &HashMap<u64, ScheduledTask>,
    deadlines: &mut BinaryHeap<Reverse<(Instant, u64)>>,
) -> Option<Instant> {
    loop {
        let Reverse((deadline, id)) = *deadlines.peek()?;
        match tasks.get(&id) {
            Some(task) if task.next_run.is_some_and(|next| next.instant == deadline) => {
                return Some(deadline);
            }
            _ => {
                deadlines.pop();
            }
        }
    }
}

async fn reap_finished_handles(handles: Vec<(String, JoinHandle<()>)>) {
    for (name, handle) in handles {
        await_finished_handle(name, handle).await;
    }
}

async fn await_finished_handle(name: String, handle: JoinHandle<()>) {
    match handle.await {
        Ok(()) => {}
        Err(err) if err.is_cancelled() => {}
        Err(err) if err.is_panic() => {
            error!(task = %name, error = %err, "Periodic task panicked");
        }
        Err(err) => {
            warn!(task = %name, error = %err, "Periodic task exited unexpectedly");
        }
    }
}

#[cfg(test)]
async fn stop_running_task(task: &mut ScheduledTask) {
    if let Some(handle) = task.running.take() {
        stop_handle(task.name.clone(), handle).await;
    }
}

fn stop_handle_with_ack(name: String, handle: JoinHandle<()>, ack: oneshot::Sender<()>) {
    tokio::spawn(async move {
        stop_handle(name, handle).await;
        let _ = ack.send(());
    });
}

fn stop_handle_detached(name: String, handle: JoinHandle<()>) {
    tokio::spawn(async move {
        stop_handle(name, handle).await;
    });
}

fn stop_handles_with_ack(handles: Vec<(String, JoinHandle<()>)>, ack: oneshot::Sender<()>) {
    if handles.is_empty() {
        let _ = ack.send(());
        return;
    }
    tokio::spawn(async move {
        stop_handles(handles).await;
        let _ = ack.send(());
    });
}

async fn stop_handle(name: String, handle: JoinHandle<()>) {
    handle.abort();
    await_finished_handle(name, handle).await;
}

async fn stop_handles(handles: Vec<(String, JoinHandle<()>)>) {
    if handles.is_empty() {
        return;
    }

    let mut waits = FuturesUnordered::new();
    for (name, handle) in handles {
        handle.abort();
        waits.push(async move {
            await_finished_handle(name, handle).await;
        });
    }

    while waits.next().await.is_some() {}
}

static GLOBAL_TASK_CENTER: OnceLock<TaskCenter> = OnceLock::new();

#[inline]
fn global_task_center() -> &'static TaskCenter {
    GLOBAL_TASK_CENTER.get_or_init(TaskCenter::new)
}

pub(crate) fn spawn_fixed<F, Fut>(
    name: impl Into<String>,
    interval: Duration,
    options: TaskOptions,
    task: F,
) -> DnsResult<ManagedTaskHandle>
where
    F: Fn(TaskRunContext) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    global_task_center().spawn_fixed(name, interval, options, task)
}

#[cfg(feature = "_task-cron")]
pub(crate) fn spawn_cron<F, Fut>(
    name: impl Into<String>,
    expression: &str,
    timezone: &str,
    options: TaskOptions,
    task: F,
) -> DnsResult<ManagedTaskHandle>
where
    F: Fn(TaskRunContext) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    global_task_center().spawn_cron(name, expression, timezone, options, task)
}

#[cfg(feature = "_task-cron")]
pub(crate) fn validate_cron(expression: &str, timezone: &str) -> DnsResult<()> {
    TaskSchedule::cron(expression, timezone).map(drop)
}

/// Stop all tasks registered in the global runtime task center.
#[inline]
pub async fn stop_all() {
    global_task_center().stop_all().await;
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use tokio::sync::{Notify, oneshot};

    use super::*;

    async fn wait_until(label: &str, condition: impl Fn() -> bool) {
        for _ in 0..128 {
            if condition() {
                return;
            }
            tokio::task::yield_now().await;
        }
        panic!("timed out waiting for {label}");
    }

    async fn flush_tasks() {
        for _ in 0..4 {
            tokio::task::yield_now().await;
        }
    }

    fn advancing_wall_clock(base_unix_ms: i64) -> WallClock {
        let base_instant = Instant::now();
        Arc::new(move || {
            let elapsed_ms = Instant::now()
                .duration_since(base_instant)
                .as_millis()
                .min(i64::MAX as u128) as i64;
            base_unix_ms.saturating_add(elapsed_ms)
        })
    }

    #[cfg(feature = "_task-cron")]
    #[derive(Debug, Clone, Copy)]
    enum TestScheduleKind {
        Fixed,
        Cron,
    }

    #[cfg(feature = "_task-cron")]
    fn spawn_test_schedule<F, Fut>(
        center: &TaskCenter,
        kind: TestScheduleKind,
        name: &str,
        options: TaskOptions,
        task: F,
    ) -> DnsResult<ManagedTaskHandle>
    where
        F: Fn(TaskRunContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        match kind {
            TestScheduleKind::Fixed => {
                center.spawn_fixed(name, Duration::from_secs(60), options, task)
            }
            TestScheduleKind::Cron => center.spawn_cron(name, "* * * * *", "UTC", options, task),
        }
    }

    async fn advance_and_flush(duration: Duration) {
        tokio::time::advance(duration).await;
        flush_tasks().await;
    }

    #[tokio::test]
    async fn isolated_build_runs_on_named_one_shot_thread() {
        let thread_name = spawn_isolated_build("snapshot-test", || {
            Ok(std::thread::current().name().map(str::to_owned))
        })
        .await
        .unwrap();
        assert_eq!(thread_name.as_deref(), Some("snapshot-test"));
    }

    #[tokio::test]
    async fn isolated_build_preserves_build_errors_and_maps_panics() {
        let error = spawn_isolated_build::<(), _>("error-test", || {
            Err(DnsError::plugin("expected build failure"))
        })
        .await
        .unwrap_err();
        assert!(error.to_string().contains("expected build failure"));

        let panic =
            spawn_isolated_build::<(), _>("panic-test", || panic!("expected isolated panic"))
                .await
                .unwrap_err();
        assert!(panic.to_string().contains("expected isolated panic"));
    }

    #[tokio::test]
    async fn stop_task_waits_for_running_future_to_abort() {
        let center = TaskCenter::new();
        let (started_tx, started_rx) = oneshot::channel();
        let released = Arc::new(AtomicBool::new(false));
        let release_flag = released.clone();
        let started_tx = Arc::new(Mutex::new(Some(started_tx)));
        let started_tx_task = started_tx.clone();
        let blocker = Arc::new(Notify::new());
        let blocker_task = blocker.clone();

        let handle = center
            .spawn_fixed(
                "test",
                Duration::from_millis(10),
                TaskOptions::default(),
                move |_| {
                    let blocker_task = blocker_task.clone();
                    let release_flag = release_flag.clone();
                    let started_tx_task = started_tx_task.clone();
                    async move {
                        struct DropFlag(Arc<AtomicBool>);
                        impl Drop for DropFlag {
                            fn drop(&mut self) {
                                self.0.store(true, Ordering::Release);
                            }
                        }

                        let _drop_flag = DropFlag(release_flag);
                        if let Some(started_tx) = started_tx_task
                            .lock()
                            .expect("started_tx mutex poisoned")
                            .take()
                        {
                            let _ = started_tx.send(());
                        }
                        blocker_task.notified().await;
                    }
                },
            )
            .unwrap();

        started_rx.await.expect("task should start");
        handle.stop().await;

        assert!(
            released.load(Ordering::Acquire),
            "running task should be aborted before stop_task returns"
        );
        blocker.notify_waiters();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn slow_task_abort_does_not_block_scheduler_actor() {
        let center = Arc::new(TaskCenter::new());
        let slow_started = Arc::new(Notify::new());
        let slow_started_task = slow_started.clone();
        let slow_handle = center
            .spawn_fixed(
                "slow-abort",
                Duration::from_secs(60),
                TaskOptions::default(),
                move |_| {
                    let slow_started_task = slow_started_task.clone();
                    async move {
                        slow_started_task.notify_one();
                        std::thread::sleep(Duration::from_millis(300));
                        std::future::pending::<()>().await;
                    }
                },
            )
            .unwrap();
        let responsive_handle = center
            .spawn_fixed(
                "responsive",
                Duration::from_secs(60),
                TaskOptions::default(),
                |_| async {},
            )
            .unwrap();

        assert_eq!(slow_handle.trigger().await, TriggerOutcome::Started);
        slow_started.notified().await;
        let stop = tokio::spawn(async move {
            slow_handle.stop().await;
        });
        tokio::time::sleep(Duration::from_millis(20)).await;

        assert_eq!(
            tokio::time::timeout(Duration::from_millis(100), responsive_handle.trigger())
                .await
                .expect("another task should remain controllable while abort is reaped"),
            TriggerOutcome::Started
        );
        tokio::time::timeout(Duration::from_secs(1), stop)
            .await
            .expect("slow task stop should eventually finish")
            .expect("stop waiter should not panic");
        responsive_handle.stop().await;
    }

    #[tokio::test(start_paused = true)]
    async fn periodic_task_does_not_overlap_and_skips_missed_ticks() {
        let center = TaskCenter::new();
        let overlap = Arc::new(AtomicBool::new(false));
        let running = Arc::new(AtomicUsize::new(0));
        let started = Arc::new(AtomicUsize::new(0));
        let blocker = Arc::new(Notify::new());
        let blocker_task = blocker.clone();
        let overlap_task = overlap.clone();
        let running_task = running.clone();
        let started_task = started.clone();

        let handle = center
            .spawn_fixed(
                "skip-test",
                Duration::from_millis(10),
                TaskOptions::default(),
                move |_| {
                    let blocker_task = blocker_task.clone();
                    let overlap_task = overlap_task.clone();
                    let running_task = running_task.clone();
                    let started_task = started_task.clone();
                    async move {
                        if running_task.fetch_add(1, Ordering::AcqRel) != 0 {
                            overlap_task.store(true, Ordering::Release);
                        }
                        started_task.fetch_add(1, Ordering::AcqRel);
                        blocker_task.notified().await;
                        running_task.fetch_sub(1, Ordering::AcqRel);
                    }
                },
            )
            .unwrap();

        flush_tasks().await;
        advance_and_flush(Duration::from_millis(10)).await;
        wait_until("first task start", || started.load(Ordering::Acquire) == 1).await;

        advance_and_flush(Duration::from_millis(100)).await;
        assert_eq!(
            started.load(Ordering::Acquire),
            1,
            "blocked periodic task should skip missed ticks instead of overlapping"
        );
        assert!(
            !overlap.load(Ordering::Acquire),
            "periodic task should never overlap with itself"
        );

        blocker.notify_waiters();
        wait_until("first task release", || {
            running.load(Ordering::Acquire) == 0
        })
        .await;

        advance_and_flush(Duration::from_millis(10)).await;
        wait_until("second task start", || started.load(Ordering::Acquire) == 2).await;

        handle.stop().await;
        assert!(
            !overlap.load(Ordering::Acquire),
            "stop_task should not introduce overlapping executions"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn stop_all_aborts_all_running_tasks_and_center_can_be_reused() {
        let center = TaskCenter::new();
        let blocker = Arc::new(Notify::new());
        let blocker_task_a = blocker.clone();
        let blocker_task_b = blocker.clone();
        let started = Arc::new(AtomicUsize::new(0));
        let started_task_a = started.clone();
        let started_task_b = started.clone();
        let released_a = Arc::new(AtomicBool::new(false));
        let released_b = Arc::new(AtomicBool::new(false));
        let released_task_a = released_a.clone();
        let released_task_b = released_b.clone();

        center
            .spawn_fixed(
                "task-a",
                Duration::from_millis(10),
                TaskOptions::default(),
                move |_| {
                    let blocker_task_a = blocker_task_a.clone();
                    let started_task_a = started_task_a.clone();
                    let released_task_a = released_task_a.clone();
                    async move {
                        struct DropFlag(Arc<AtomicBool>);
                        impl Drop for DropFlag {
                            fn drop(&mut self) {
                                self.0.store(true, Ordering::Release);
                            }
                        }

                        let _drop_flag = DropFlag(released_task_a);
                        started_task_a.fetch_add(1, Ordering::AcqRel);
                        blocker_task_a.notified().await;
                    }
                },
            )
            .unwrap();
        center
            .spawn_fixed(
                "task-b",
                Duration::from_millis(10),
                TaskOptions::default(),
                move |_| {
                    let blocker_task_b = blocker_task_b.clone();
                    let started_task_b = started_task_b.clone();
                    let released_task_b = released_task_b.clone();
                    async move {
                        struct DropFlag(Arc<AtomicBool>);
                        impl Drop for DropFlag {
                            fn drop(&mut self) {
                                self.0.store(true, Ordering::Release);
                            }
                        }

                        let _drop_flag = DropFlag(released_task_b);
                        started_task_b.fetch_add(1, Ordering::AcqRel);
                        blocker_task_b.notified().await;
                    }
                },
            )
            .unwrap();

        flush_tasks().await;
        advance_and_flush(Duration::from_millis(10)).await;
        wait_until("both tasks start", || started.load(Ordering::Acquire) == 2).await;

        center.stop_all().await;
        assert!(
            released_a.load(Ordering::Acquire) && released_b.load(Ordering::Acquire),
            "stop_all should abort every running task before returning"
        );

        let rerun_count = Arc::new(AtomicUsize::new(0));
        let rerun_count_task = rerun_count.clone();
        let rerun_handle = center
            .spawn_fixed(
                "task-c",
                Duration::from_millis(10),
                TaskOptions::default(),
                move |_| {
                    let rerun_count_task = rerun_count_task.clone();
                    async move {
                        rerun_count_task.fetch_add(1, Ordering::AcqRel);
                    }
                },
            )
            .unwrap();

        flush_tasks().await;
        advance_and_flush(Duration::from_millis(10)).await;
        wait_until("task center reuse", || {
            rerun_count.load(Ordering::Acquire) == 1
        })
        .await;
        rerun_handle.stop().await;
        blocker.notify_waiters();
    }

    #[tokio::test(start_paused = true)]
    async fn handle_stop_detached_removes_only_target_task() {
        let center = TaskCenter::new();
        let stopped_count = Arc::new(AtomicUsize::new(0));
        let kept_count = Arc::new(AtomicUsize::new(0));
        let stopped_count_task = stopped_count.clone();
        let kept_count_task = kept_count.clone();

        let stopped_handle = center
            .spawn_fixed(
                "stopped",
                Duration::from_millis(10),
                TaskOptions::default(),
                move |_| {
                    let stopped_count_task = stopped_count_task.clone();
                    async move {
                        stopped_count_task.fetch_add(1, Ordering::AcqRel);
                    }
                },
            )
            .unwrap();
        let kept_handle = center
            .spawn_fixed(
                "kept",
                Duration::from_millis(10),
                TaskOptions::default(),
                move |_| {
                    let kept_count_task = kept_count_task.clone();
                    async move {
                        kept_count_task.fetch_add(1, Ordering::AcqRel);
                    }
                },
            )
            .unwrap();

        flush_tasks().await;
        advance_and_flush(Duration::from_millis(10)).await;
        wait_until("initial task runs", || {
            stopped_count.load(Ordering::Acquire) == 1 && kept_count.load(Ordering::Acquire) == 1
        })
        .await;

        stopped_handle.stop_detached();
        flush_tasks().await;
        for expected in 2..=4 {
            advance_and_flush(Duration::from_millis(10)).await;
            wait_until("surviving task run", || {
                kept_count.load(Ordering::Acquire) == expected
            })
            .await;
            assert_eq!(
                stopped_count.load(Ordering::Acquire),
                1,
                "detached stop should remove only the targeted task"
            );
        }

        kept_handle.stop().await;
    }

    #[tokio::test(start_paused = true)]
    async fn stop_task_before_first_run_prevents_execution() {
        let center = TaskCenter::new();
        let started = Arc::new(AtomicUsize::new(0));
        let started_task = started.clone();

        let handle = center
            .spawn_fixed(
                "pending",
                Duration::from_millis(10),
                TaskOptions::default(),
                move |_| {
                    let started_task = started_task.clone();
                    async move {
                        started_task.fetch_add(1, Ordering::AcqRel);
                    }
                },
            )
            .unwrap();

        handle.stop().await;
        advance_and_flush(Duration::from_millis(100)).await;

        assert_eq!(
            started.load(Ordering::Acquire),
            0,
            "stop_task should remove a pending task before its first scheduled run"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn panicked_task_is_reaped_and_rescheduled_on_next_tick() {
        let center = TaskCenter::new();
        let attempts = Arc::new(AtomicUsize::new(0));
        let successes = Arc::new(AtomicUsize::new(0));
        let attempts_task = attempts.clone();
        let successes_task = successes.clone();

        let handle = center
            .spawn_fixed(
                "panic-once",
                Duration::from_millis(10),
                TaskOptions::default(),
                move |_| {
                    let attempts_task = attempts_task.clone();
                    let successes_task = successes_task.clone();
                    async move {
                        let attempt = attempts_task.fetch_add(1, Ordering::AcqRel) + 1;
                        if attempt == 1 {
                            panic!("intentional panic for scheduler recovery test");
                        }
                        successes_task.fetch_add(1, Ordering::AcqRel);
                    }
                },
            )
            .unwrap();

        flush_tasks().await;
        advance_and_flush(Duration::from_millis(10)).await;
        wait_until("first task attempt", || {
            attempts.load(Ordering::Acquire) == 1
        })
        .await;

        advance_and_flush(Duration::from_millis(10)).await;
        wait_until("second task attempt", || {
            attempts.load(Ordering::Acquire) == 2
        })
        .await;
        wait_until("successful rerun after panic", || {
            successes.load(Ordering::Acquire) == 1
        })
        .await;

        handle.stop().await;
    }

    #[tokio::test(start_paused = true)]
    async fn managed_task_manual_trigger_rejects_overlap_and_stops_cleanly() {
        let center = TaskCenter::new();
        let started = Arc::new(Notify::new());
        let blocker = Arc::new(Notify::new());
        let released = Arc::new(AtomicBool::new(false));
        let contexts = Arc::new(Mutex::new(Vec::new()));
        let skipped = Arc::new(AtomicUsize::new(0));

        let started_task = started.clone();
        let blocker_task = blocker.clone();
        let released_task = released.clone();
        let contexts_task = contexts.clone();
        let skipped_event = skipped.clone();
        let on_event: TaskEventHandler = Arc::new(move |_| {
            skipped_event.fetch_add(1, Ordering::Relaxed);
        });

        let handle = center
            .spawn_fixed(
                "managed-manual",
                Duration::from_secs(60),
                TaskOptions::default().on_event(on_event),
                move |context| {
                    let started_task = started_task.clone();
                    let blocker_task = blocker_task.clone();
                    let released_task = released_task.clone();
                    let contexts_task = contexts_task.clone();
                    async move {
                        struct DropFlag(Arc<AtomicBool>);
                        impl Drop for DropFlag {
                            fn drop(&mut self) {
                                self.0.store(true, Ordering::Release);
                            }
                        }

                        let _drop_flag = DropFlag(released_task);
                        contexts_task.lock().unwrap().push(context);
                        started_task.notify_one();
                        blocker_task.notified().await;
                    }
                },
            )
            .unwrap();

        assert_eq!(handle.trigger().await, TriggerOutcome::Started);
        started.notified().await;
        assert_eq!(
            handle.trigger().await,
            TriggerOutcome::AlreadyRunning,
            "manual executions must be rejected instead of queued"
        );
        assert_eq!(skipped.load(Ordering::Relaxed), 1);
        assert_eq!(contexts.lock().unwrap()[0].trigger, TaskTriggerKind::Manual);

        advance_and_flush(Duration::from_secs(60)).await;
        wait_until("scheduled overlap to be skipped", || {
            skipped.load(Ordering::Relaxed) == 2
        })
        .await;
        assert_eq!(
            contexts.lock().unwrap().len(),
            1,
            "scheduled and manual execution must share one running slot"
        );

        handle.stop().await;
        assert!(released.load(Ordering::Acquire));
        assert_eq!(handle.trigger().await, TriggerOutcome::Unavailable);
        blocker.notify_waiters();
    }

    #[tokio::test(start_paused = true)]
    async fn manual_trigger_preserves_fixed_interval_schedule() {
        let center = TaskCenter::new();
        let runs = Arc::new(AtomicUsize::new(0));
        let triggers = Arc::new(Mutex::new(Vec::new()));
        let runs_task = runs.clone();
        let triggers_task = triggers.clone();
        let handle = center
            .spawn_fixed(
                "manual-preserve",
                Duration::from_secs(10),
                TaskOptions::default(),
                move |context| {
                    let runs_task = runs_task.clone();
                    let triggers_task = triggers_task.clone();
                    async move {
                        runs_task.fetch_add(1, Ordering::Relaxed);
                        triggers_task.lock().unwrap().push(context.trigger);
                    }
                },
            )
            .unwrap();

        advance_and_flush(Duration::from_secs(5)).await;
        assert_eq!(handle.trigger().await, TriggerOutcome::Started);
        wait_until("manual run", || runs.load(Ordering::Relaxed) == 1).await;

        advance_and_flush(Duration::from_secs(5)).await;
        wait_until("original fixed deadline", || {
            runs.load(Ordering::Relaxed) == 2
        })
        .await;
        advance_and_flush(Duration::from_secs(10)).await;
        wait_until("next fixed deadline", || runs.load(Ordering::Relaxed) == 3).await;
        assert_eq!(
            triggers.lock().unwrap().as_slice(),
            &[
                TaskTriggerKind::Manual,
                TaskTriggerKind::Scheduled,
                TaskTriggerKind::Scheduled
            ]
        );
        handle.stop().await;
    }

    #[cfg(feature = "_task-cron")]
    #[test]
    fn cron_schedule_computes_timezone_aware_future_deadlines() {
        let instant = Instant::now();
        let epoch = TaskDeadline {
            instant,
            unix_ms: 0,
        };

        let utc = TaskSchedule::cron("0 1 * * *", "UTC").unwrap();
        let utc_next = utc.next_after(epoch, epoch).unwrap().unwrap();
        assert_eq!(utc_next.unix_ms, 3_600_000);

        let shanghai = TaskSchedule::cron("0 9 * * *", "Asia/Shanghai").unwrap();
        let shanghai_next = shanghai.next_after(epoch, epoch).unwrap().unwrap();
        assert_eq!(shanghai_next.unix_ms, 3_600_000);
    }

    #[cfg(feature = "_task-cron")]
    #[test]
    fn cron_schedule_handles_day_boundaries_and_dst_gaps() {
        fn deadline(timestamp: &str, instant: Instant) -> TaskDeadline {
            TaskDeadline {
                instant,
                unix_ms: timestamp.parse::<Timestamp>().unwrap().as_millisecond(),
            }
        }

        let instant = Instant::now();
        let year_end = deadline("2024-12-31T23:59:00Z", instant);
        let midnight = TaskSchedule::cron("0 0 * * *", "UTC")
            .unwrap()
            .next_after(year_end, year_end)
            .unwrap()
            .unwrap();
        assert_eq!(
            Timestamp::from_millisecond(midnight.unix_ms).unwrap(),
            "2025-01-01T00:00:00Z".parse::<Timestamp>().unwrap()
        );

        let before_dst_gap = deadline("2024-03-10T06:59:00Z", instant);
        let after_dst_gap = TaskSchedule::cron("30 2 * * *", "America/New_York")
            .unwrap()
            .next_after(before_dst_gap, before_dst_gap)
            .unwrap()
            .unwrap();
        assert_eq!(
            Timestamp::from_millisecond(after_dst_gap.unix_ms).unwrap(),
            "2024-03-11T06:30:00Z".parse::<Timestamp>().unwrap()
        );
    }

    #[test]
    fn fixed_schedule_skips_large_missed_window_without_tick_iteration() {
        let instant = Instant::now();
        let previous = TaskDeadline {
            instant,
            unix_ms: 0,
        };
        let now = TaskDeadline {
            instant: instant + Duration::from_secs(10_000),
            unix_ms: 10_000_000,
        };
        let next = TaskSchedule::fixed(Duration::from_nanos(1))
            .next_after(previous, now)
            .unwrap()
            .unwrap();

        assert_eq!(next.instant, now.instant + Duration::from_nanos(1));
        assert!(next.instant > now.instant);
    }

    #[cfg(feature = "_task-cron")]
    #[tokio::test(start_paused = true)]
    async fn cron_deadline_reanchors_after_wall_clock_jumps() {
        let wall_unix_ms = Arc::new(std::sync::atomic::AtomicI64::new(0));
        let wall_clock_value = wall_unix_ms.clone();
        let center =
            TaskCenter::with_wall_clock(Arc::new(move || wall_clock_value.load(Ordering::Acquire)));
        let contexts = Arc::new(Mutex::new(Vec::new()));
        let contexts_task = contexts.clone();
        let handle = center
            .spawn_cron(
                "wall-clock-jump",
                "* * * * *",
                "UTC",
                TaskOptions::default(),
                move |context| {
                    let contexts_task = contexts_task.clone();
                    async move {
                        contexts_task.lock().unwrap().push(context);
                    }
                },
            )
            .unwrap();

        wall_unix_ms.store(-60_000, Ordering::Release);
        advance_and_flush(Duration::from_secs(60)).await;
        assert!(
            contexts.lock().unwrap().is_empty(),
            "a backward wall-clock jump must postpone the calendar deadline"
        );

        wall_unix_ms.store(120_000, Ordering::Release);
        advance_and_flush(CRON_WALL_CLOCK_RECHECK_INTERVAL).await;
        wait_until("forward wall-clock jump", || {
            contexts.lock().unwrap().len() == 1
        })
        .await;
        let context = contexts.lock().unwrap()[0];
        assert_eq!(context.trigger, TaskTriggerKind::Scheduled);
        assert_eq!(context.scheduled_at_unix_ms, 60_000);
        handle.stop().await;
    }

    #[cfg(feature = "_task-cron")]
    #[test]
    fn cron_reanchor_rebuilds_deadline_heap_without_stale_growth() {
        AppClock::start();
        let clock = TaskClock::new(Arc::new(|| 0));
        let schedule = TaskSchedule::cron("* * * * *", "UTC").unwrap();
        let now = clock.now_for(&schedule);
        let next_run = schedule.next_after(now, now).unwrap();
        let task: TaskFn = Arc::new(|_| Box::pin(async {}));
        let mut tasks = HashMap::from([(
            1,
            ScheduledTask {
                name: "bounded-cron-heap".to_string(),
                clock,
                schedule,
                next_run,
                task,
                on_event: None,
                running: None,
            },
        )]);
        let mut deadlines = BinaryHeap::new();
        deadlines.push(Reverse((next_run.unwrap().instant, 1)));

        for _ in 0..1_000 {
            refresh_cron_deadlines(&mut tasks, &mut deadlines);
        }

        assert_eq!(deadlines.len(), 1);
        assert_eq!(
            next_deadline(&tasks, &mut deadlines),
            tasks[&1].next_run.map(|d| d.instant)
        );
    }

    #[cfg(feature = "_task-cron")]
    #[tokio::test]
    async fn fixed_and_cron_share_managed_lifecycle_capabilities() {
        for kind in [TestScheduleKind::Fixed, TestScheduleKind::Cron] {
            let center = TaskCenter::with_wall_clock(Arc::new(|| 0));
            let started = Arc::new(Notify::new());
            let blocker = Arc::new(Notify::new());
            let skipped = Arc::new(AtomicUsize::new(0));
            let started_task = started.clone();
            let blocker_task = blocker.clone();
            let skipped_event = skipped.clone();
            let handle = spawn_test_schedule(
                &center,
                kind,
                "symmetric-running",
                TaskOptions::default().on_event(Arc::new(move |_| {
                    skipped_event.fetch_add(1, Ordering::Relaxed);
                })),
                move |context| {
                    let started_task = started_task.clone();
                    let blocker_task = blocker_task.clone();
                    async move {
                        assert_eq!(context.trigger, TaskTriggerKind::Manual);
                        started_task.notify_one();
                        blocker_task.notified().await;
                    }
                },
            )
            .unwrap();

            assert_eq!(handle.trigger().await, TriggerOutcome::Started, "{kind:?}");
            started.notified().await;
            assert_eq!(
                handle.trigger().await,
                TriggerOutcome::AlreadyRunning,
                "{kind:?}"
            );
            assert_eq!(skipped.load(Ordering::Relaxed), 1, "{kind:?}");
            handle.stop().await;
            assert_eq!(
                handle.trigger().await,
                TriggerOutcome::Unavailable,
                "{kind:?}"
            );
            blocker.notify_waiters();

            let detached = spawn_test_schedule(
                &center,
                kind,
                "symmetric-detached",
                TaskOptions::default(),
                |_| async {},
            )
            .unwrap();
            detached.stop_detached();
            flush_tasks().await;
            assert_eq!(
                detached.trigger().await,
                TriggerOutcome::Unavailable,
                "{kind:?}"
            );

            let attempts = Arc::new(AtomicUsize::new(0));
            let attempts_task = attempts.clone();
            let panic_handle = spawn_test_schedule(
                &center,
                kind,
                "symmetric-panic",
                TaskOptions::default(),
                move |_| {
                    let attempts_task = attempts_task.clone();
                    async move {
                        if attempts_task.fetch_add(1, Ordering::AcqRel) == 0 {
                            panic!("intentional symmetric task panic");
                        }
                    }
                },
            )
            .unwrap();
            assert_eq!(panic_handle.trigger().await, TriggerOutcome::Started);
            wait_until("first symmetric panic attempt", || {
                attempts.load(Ordering::Acquire) == 1
            })
            .await;
            assert_eq!(panic_handle.trigger().await, TriggerOutcome::Started);
            wait_until("second symmetric panic attempt", || {
                attempts.load(Ordering::Acquire) == 2
            })
            .await;
            panic_handle.stop().await;
        }
    }

    #[cfg(feature = "_task-cron")]
    #[tokio::test(start_paused = true)]
    async fn cron_entrypoint_supports_manual_and_scheduled_contexts() {
        let center = TaskCenter::with_wall_clock(advancing_wall_clock(0));
        let contexts = Arc::new(Mutex::new(Vec::new()));
        let contexts_task = contexts.clone();
        let handle = center
            .spawn_cron(
                "cron-context",
                "* * * * *",
                "UTC",
                TaskOptions::default(),
                move |context| {
                    let contexts_task = contexts_task.clone();
                    async move {
                        contexts_task.lock().unwrap().push(context);
                    }
                },
            )
            .unwrap();

        assert_eq!(handle.trigger().await, TriggerOutcome::Started);
        wait_until("manual cron run", || !contexts.lock().unwrap().is_empty()).await;
        advance_and_flush(Duration::from_secs(61)).await;
        wait_until("scheduled cron run", || contexts.lock().unwrap().len() >= 2).await;

        let triggers = contexts
            .lock()
            .unwrap()
            .iter()
            .map(|context| context.trigger)
            .collect::<Vec<_>>();
        assert_eq!(
            triggers,
            [TaskTriggerKind::Manual, TaskTriggerKind::Scheduled]
        );
        handle.stop().await;
        assert_eq!(handle.trigger().await, TriggerOutcome::Unavailable);
    }

    #[tokio::test]
    async fn fixed_entrypoint_rejects_zero_interval() {
        let center = TaskCenter::new();
        assert!(
            center
                .spawn_fixed("zero", Duration::ZERO, TaskOptions::default(), |_| async {},)
                .is_err()
        );
    }

    #[cfg(feature = "_task-cron")]
    #[tokio::test]
    async fn cron_entrypoint_validates_schedule_inputs() {
        let center = TaskCenter::new();
        assert!(
            center
                .spawn_cron(
                    "invalid-cron",
                    "0 0 * * * *",
                    "UTC",
                    TaskOptions::default(),
                    |_| async {},
                )
                .is_err()
        );
        assert!(
            center
                .spawn_cron(
                    "invalid-timezone",
                    "0 0 * * *",
                    "Not/AZone",
                    TaskOptions::default(),
                    |_| async {},
                )
                .is_err()
        );
    }

    #[tokio::test]
    async fn repeated_manual_triggers_preserve_fixed_deadline_and_heap_size() {
        let schedule = TaskSchedule::fixed(Duration::from_secs(3600));
        let clock = TaskClock::new(advancing_wall_clock(0));
        let now = clock.now_for(&schedule);
        let next_run = schedule.next_after(now, now).unwrap();
        let task: TaskFn = Arc::new(|_| Box::pin(async {}));
        let mut tasks = HashMap::from([(
            1,
            ScheduledTask {
                name: "fixed-preserve".to_string(),
                clock,
                schedule,
                next_run,
                task,
                on_event: None,
                running: None,
            },
        )]);
        let mut deadlines = BinaryHeap::new();
        let deadline = next_run.unwrap();
        deadlines.push(Reverse((deadline.instant, 1)));
        for _ in 0..128 {
            let (reply, response) = oneshot::channel();
            handle_trigger(TriggerCommand { id: 1, reply }, &mut tasks).await;
            assert_eq!(response.await.unwrap(), TriggerOutcome::Started);
            assert_eq!(tasks[&1].next_run, Some(deadline));
            stop_running_task(tasks.get_mut(&1).unwrap()).await;
        }

        assert_eq!(deadlines.len(), 1);
        assert_eq!(
            next_deadline(&tasks, &mut deadlines),
            Some(deadline.instant)
        );
    }

    #[test]
    fn removing_task_compacts_stale_deadline_entries() {
        AppClock::start();
        let clock = TaskClock::new(advancing_wall_clock(0));
        let schedule = TaskSchedule::fixed(Duration::from_secs(3600));
        let now = clock.now_for(&schedule);
        let next_run = schedule.next_after(now, now).unwrap();
        let task: TaskFn = Arc::new(|_| Box::pin(async {}));
        let make_task = |name: &str| ScheduledTask {
            name: name.to_string(),
            clock: clock.clone(),
            schedule: schedule.clone(),
            next_run,
            task: task.clone(),
            on_event: None,
            running: None,
        };
        let mut tasks = HashMap::from([(1, make_task("removed")), (2, make_task("kept"))]);
        let deadline = next_run.unwrap();
        let mut deadlines = BinaryHeap::from([
            Reverse((deadline.instant, 1)),
            Reverse((deadline.instant, 2)),
        ]);
        let (ack, mut response) = oneshot::channel();

        handle_command(Command::Remove { id: 1, ack }, &mut tasks, &mut deadlines);

        assert_eq!(response.try_recv(), Ok(()));
        assert_eq!(tasks.len(), 1);
        assert_eq!(deadlines.len(), 1);
        assert_eq!(deadlines.peek(), Some(&Reverse((deadline.instant, 2))));
    }

    #[tokio::test]
    async fn managed_trigger_reports_unavailable_when_queue_is_full() {
        let (control_tx, _control_rx) = mpsc::unbounded_channel();
        let (trigger_tx, mut trigger_rx) = mpsc::channel(1);
        let (reply, _response) = oneshot::channel();
        trigger_tx
            .try_send(TriggerCommand { id: 1, reply })
            .unwrap();
        let handle = ManagedTaskHandle {
            id: 2,
            control_tx,
            trigger_tx,
        };

        assert_eq!(handle.trigger().await, TriggerOutcome::Unavailable);
        trigger_rx
            .recv()
            .await
            .expect("queued trigger should remain");
    }

    #[tokio::test]
    async fn registration_retries_once_after_channel_close_race() {
        let center = TaskCenter::new();
        let (control_tx, control_rx) = mpsc::unbounded_channel();
        let (trigger_tx, _trigger_rx) = mpsc::channel(1);
        let channels = TaskCenterChannels {
            control_tx,
            trigger_tx,
        };
        *center.channels.lock().unwrap() = channels.clone();

        let clock = TaskClock::new(advancing_wall_clock(0));
        let now = clock.monotonic_now();
        let schedule = TaskSchedule::fixed(Duration::from_secs(60));
        let task: TaskFn = Arc::new(|_| Box::pin(async {}));
        let command = Command::Register {
            id: 1,
            name: "retry-registration".to_string(),
            clock,
            next_run: schedule.next_after(now, now).unwrap(),
            schedule,
            task,
            on_event: None,
        };
        drop(control_rx);

        let retry_channels = center.send_registration(channels, command).unwrap();
        let handle = ManagedTaskHandle {
            id: 1,
            control_tx: retry_channels.control_tx,
            trigger_tx: retry_channels.trigger_tx,
        };
        assert_eq!(handle.trigger().await, TriggerOutcome::Started);
        handle.stop().await;
    }

    #[test]
    fn task_center_restarts_scheduler_after_runtime_replacement() {
        let first_runtime = tokio::runtime::Runtime::new().unwrap();
        let center = first_runtime.block_on(async { TaskCenter::new() });
        drop(first_runtime);

        let second_runtime = tokio::runtime::Runtime::new().unwrap();
        second_runtime.block_on(async {
            let runs = Arc::new(AtomicUsize::new(0));
            let runs_task = runs.clone();
            let handle = center
                .spawn_fixed(
                    "runtime-replacement",
                    Duration::from_secs(60),
                    TaskOptions::default(),
                    move |_| {
                        let runs_task = runs_task.clone();
                        async move {
                            runs_task.fetch_add(1, Ordering::Relaxed);
                        }
                    },
                )
                .unwrap();

            assert_eq!(handle.trigger().await, TriggerOutcome::Started);
            wait_until("run after scheduler restart", || {
                runs.load(Ordering::Relaxed) == 1
            })
            .await;
            handle.stop().await;
        });
    }
}
