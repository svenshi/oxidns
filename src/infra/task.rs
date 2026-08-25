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
#[cfg(feature = "plugin-cron")]
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

type TaskFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;
type TaskFn = Arc<dyn Fn(TaskRunContext) -> TaskFuture + Send + Sync + 'static>;
pub(crate) type TaskEventHandler = Arc<dyn Fn(TaskEvent) + Send + Sync + 'static>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TaskTriggerKind {
    Scheduled,
    Manual,
}

#[derive(Debug, Clone, Copy)]
#[cfg_attr(not(feature = "plugin-cron"), allow(dead_code))]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TaskDeadline {
    instant: Instant,
    unix_ms: i64,
}

#[derive(Debug, Clone, Copy)]
struct TaskClock {
    base_instant: Instant,
    base_unix_ms: i64,
}

impl TaskClock {
    fn new() -> Self {
        Self {
            base_instant: Instant::now(),
            base_unix_ms: AppClock::now_timestamp().min(i64::MAX as u64) as i64,
        }
    }

    fn now(self) -> TaskDeadline {
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
}

#[derive(Debug, Clone)]
pub(crate) enum TaskSchedule {
    FixedInterval {
        interval: Duration,
    },
    #[cfg(feature = "plugin-cron")]
    Cron {
        expression: String,
        timezone: String,
        crontab: Arc<cronexpr::Crontab>,
    },
}

impl TaskSchedule {
    pub(crate) fn fixed(interval: Duration) -> Self {
        Self::FixedInterval { interval }
    }

    #[cfg(feature = "plugin-cron")]
    pub(crate) fn cron(expression: &str, timezone: &str) -> DnsResult<Self> {
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
                let interval_ms = interval.as_millis().min(i64::MAX as u128) as i64;
                let mut next = TaskDeadline {
                    instant: previous.instant + *interval,
                    unix_ms: previous.unix_ms.saturating_add(interval_ms),
                };
                while next.instant <= now.instant {
                    next.instant += *interval;
                    next.unix_ms = next.unix_ms.saturating_add(interval_ms);
                }
                Ok(Some(next))
            }
            #[cfg(feature = "plugin-cron")]
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

    fn resets_after_manual_run(&self) -> bool {
        matches!(self, Self::FixedInterval { .. })
    }
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
        ack: Option<oneshot::Sender<()>>,
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
#[derive(Debug)]
pub struct TaskCenter {
    next_id: AtomicU64,
    channels: StdMutex<TaskCenterChannels>,
}

#[derive(Debug, Clone)]
struct TaskCenterChannels {
    control_tx: mpsc::UnboundedSender<Command>,
    trigger_tx: mpsc::Sender<TriggerCommand>,
}

impl TaskCenter {
    fn new() -> Self {
        AppClock::start();
        Self {
            next_id: AtomicU64::new(1),
            channels: StdMutex::new(Self::spawn_scheduler()),
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

    /// Register a fixed-interval task into the global scheduler.
    ///
    /// The first run happens after one full interval, and missed ticks use
    /// Skip behavior semantics (no burst catch-up).
    pub fn spawn_fixed<F, Fut>(&self, name: impl Into<String>, interval: Duration, task: F) -> u64
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let name = name.into();
        let task: TaskFn = Arc::new(move |_| Box::pin(task()));
        let clock = TaskClock::new();
        let now = clock.now();
        let schedule = TaskSchedule::fixed(interval);
        let next_run = schedule.next_after(now, now).ok().flatten();
        let channels = self.channels();
        let _ = channels.control_tx.send(Command::Register {
            id,
            name,
            clock,
            schedule,
            next_run,
            task,
            on_event: None,
            ack: None,
        });
        id
    }

    #[cfg_attr(not(feature = "plugin-cron"), allow(dead_code))]
    async fn register_scheduled<F, Fut>(
        &self,
        name: impl Into<String>,
        schedule: TaskSchedule,
        on_event: Option<TaskEventHandler>,
        task: F,
    ) -> DnsResult<ManagedTaskHandle>
    where
        F: Fn(TaskRunContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let name = name.into();
        let clock = TaskClock::new();
        let now = clock.now();
        let next_run = compute_next_deadline(&name, &schedule, now, now)?;
        let task: TaskFn = Arc::new(move |context| Box::pin(task(context)));
        let (ack_tx, ack_rx) = oneshot::channel();
        let channels = self.channels();
        channels
            .control_tx
            .send(Command::Register {
                id,
                name,
                clock,
                schedule,
                next_run,
                task,
                on_event,
                ack: Some(ack_tx),
            })
            .map_err(|_| DnsError::runtime("task center is not running"))?;
        ack_rx
            .await
            .map_err(|_| DnsError::runtime("task center stopped during registration"))?;
        Ok(ManagedTaskHandle {
            id,
            control_tx: channels.control_tx,
            trigger_tx: channels.trigger_tx,
        })
    }

    /// Stop a previously registered task.
    pub async fn stop_task(&self, id: u64) {
        let (ack_tx, ack_rx) = oneshot::channel();
        if self
            .channels()
            .control_tx
            .send(Command::Remove { id, ack: ack_tx })
            .is_err()
        {
            return;
        }
        let _ = ack_rx.await;
    }

    /// Request task removal without waiting for acknowledgement.
    ///
    /// This is useful from synchronous drop paths where awaiting is impossible.
    pub fn stop_task_detached(&self, id: u64) {
        let _ = self
            .channels()
            .control_tx
            .send(Command::RemoveDetached { id });
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

#[derive(Clone)]
#[cfg_attr(not(feature = "plugin-cron"), allow(dead_code))]
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
    #[cfg_attr(not(feature = "plugin-cron"), allow(dead_code))]
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

    #[cfg_attr(not(feature = "plugin-cron"), allow(dead_code))]
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
}

#[hotpath::measure]
async fn run_scheduler(
    mut control_rx: mpsc::UnboundedReceiver<Command>,
    mut trigger_rx: mpsc::Receiver<TriggerCommand>,
) {
    let mut tasks: HashMap<u64, ScheduledTask> = HashMap::new();
    let mut deadlines: BinaryHeap<Reverse<(Instant, u64)>> = BinaryHeap::new();

    loop {
        if let Some(next_deadline) = next_deadline(&tasks, &mut deadlines) {
            tokio::select! {
                biased;
                command = control_rx.recv() => {
                    let Some(command) = command else {
                        break;
                    };
                    handle_command(command, &mut tasks, &mut deadlines).await;
                }
                _ = sleep_until(next_deadline) => {
                    run_due_tasks(&mut tasks, &mut deadlines).await;
                }
                trigger = trigger_rx.recv() => {
                    if let Some(trigger) = trigger {
                        handle_trigger(trigger, &mut tasks, &mut deadlines).await;
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
                    handle_command(command, &mut tasks, &mut deadlines).await;
                }
                trigger = trigger_rx.recv() => {
                    if let Some(trigger) = trigger {
                        handle_trigger(trigger, &mut tasks, &mut deadlines).await;
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

async fn handle_command(
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
            ack,
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
            if let Some(ack) = ack {
                let _ = ack.send(());
            }
        }
        Command::Remove { id, ack } => {
            if let Some(mut task) = tasks.remove(&id) {
                stop_running_task(&mut task).await;
            }
            let _ = ack.send(());
        }
        Command::RemoveDetached { id } => {
            if let Some(mut task) = tasks.remove(&id) {
                stop_running_task(&mut task).await;
            }
        }
        Command::StopAll { ack } => {
            let mut running = Vec::new();
            for (_, mut task) in tasks.drain() {
                if let Some(handle) = task.running.take() {
                    running.push((task.name, handle));
                }
            }
            deadlines.clear();
            stop_handles(running).await;
            let _ = ack.send(());
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
            let task_now = task.clock.now();
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

async fn handle_trigger(
    trigger: TriggerCommand,
    tasks: &mut HashMap<u64, ScheduledTask>,
    deadlines: &mut BinaryHeap<Reverse<(Instant, u64)>>,
) {
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

    let now = task.clock.now();
    if task.schedule.resets_after_manual_run() {
        match compute_next_deadline(&task.name, &task.schedule, now, now) {
            Ok(next_run) => {
                task.next_run = next_run;
                if let Some(next_run) = next_run {
                    deadlines.push(Reverse((next_run.instant, trigger.id)));
                }
            }
            Err(error) => {
                task.next_run = None;
                error!(
                    task = %task.name,
                    error = %error,
                    "Failed to reset scheduled task after manual trigger; automatic execution is paused"
                );
            }
        }
    }

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

async fn stop_running_task(task: &mut ScheduledTask) {
    if let Some(handle) = task.running.take() {
        stop_handle(task.name.clone(), handle).await;
    }
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

/// Register a fixed-interval task into the global runtime task center.
#[inline]
pub fn spawn_fixed<F, Fut>(name: impl Into<String>, interval: Duration, task: F) -> u64
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    global_task_center().spawn_fixed(name, interval, task)
}

#[cfg_attr(not(feature = "plugin-cron"), allow(dead_code))]
pub(crate) async fn register_scheduled<F, Fut>(
    name: impl Into<String>,
    schedule: TaskSchedule,
    on_event: Option<TaskEventHandler>,
    task: F,
) -> DnsResult<ManagedTaskHandle>
where
    F: Fn(TaskRunContext) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    global_task_center()
        .register_scheduled(name, schedule, on_event, task)
        .await
}

/// Stop a previously registered global task.
#[inline]
pub async fn stop_task(id: u64) {
    global_task_center().stop_task(id).await;
}

/// Stop a previously registered global task from a synchronous path.
#[inline]
pub fn stop_task_detached(id: u64) {
    global_task_center().stop_task_detached(id);
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

        let task_id = center.spawn_fixed("test", Duration::from_millis(10), move || {
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
        });

        started_rx.await.expect("task should start");
        center.stop_task(task_id).await;

        assert!(
            released.load(Ordering::Acquire),
            "running task should be aborted before stop_task returns"
        );
        blocker.notify_waiters();
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

        let task_id = center.spawn_fixed("skip-test", Duration::from_millis(10), move || {
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
        });

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

        center.stop_task(task_id).await;
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

        center.spawn_fixed("task-a", Duration::from_millis(10), move || {
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
        });
        center.spawn_fixed("task-b", Duration::from_millis(10), move || {
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
        });

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
        let rerun_id = center.spawn_fixed("task-c", Duration::from_millis(10), move || {
            let rerun_count_task = rerun_count_task.clone();
            async move {
                rerun_count_task.fetch_add(1, Ordering::AcqRel);
            }
        });

        flush_tasks().await;
        advance_and_flush(Duration::from_millis(10)).await;
        wait_until("task center reuse", || {
            rerun_count.load(Ordering::Acquire) == 1
        })
        .await;
        center.stop_task(rerun_id).await;
        blocker.notify_waiters();
    }

    #[tokio::test(start_paused = true)]
    async fn stop_task_detached_removes_only_target_task() {
        let center = TaskCenter::new();
        let stopped_count = Arc::new(AtomicUsize::new(0));
        let kept_count = Arc::new(AtomicUsize::new(0));
        let stopped_count_task = stopped_count.clone();
        let kept_count_task = kept_count.clone();

        let stopped_id = center.spawn_fixed("stopped", Duration::from_millis(10), move || {
            let stopped_count_task = stopped_count_task.clone();
            async move {
                stopped_count_task.fetch_add(1, Ordering::AcqRel);
            }
        });
        let kept_id = center.spawn_fixed("kept", Duration::from_millis(10), move || {
            let kept_count_task = kept_count_task.clone();
            async move {
                kept_count_task.fetch_add(1, Ordering::AcqRel);
            }
        });

        flush_tasks().await;
        advance_and_flush(Duration::from_millis(10)).await;
        wait_until("initial task runs", || {
            stopped_count.load(Ordering::Acquire) == 1 && kept_count.load(Ordering::Acquire) == 1
        })
        .await;

        center.stop_task_detached(stopped_id);
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

        center.stop_task(kept_id).await;
    }

    #[tokio::test(start_paused = true)]
    async fn stop_task_before_first_run_prevents_execution() {
        let center = TaskCenter::new();
        let started = Arc::new(AtomicUsize::new(0));
        let started_task = started.clone();

        let task_id = center.spawn_fixed("pending", Duration::from_millis(10), move || {
            let started_task = started_task.clone();
            async move {
                started_task.fetch_add(1, Ordering::AcqRel);
            }
        });

        flush_tasks().await;
        center.stop_task(task_id).await;
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

        let task_id = center.spawn_fixed("panic-once", Duration::from_millis(10), move || {
            let attempts_task = attempts_task.clone();
            let successes_task = successes_task.clone();
            async move {
                let attempt = attempts_task.fetch_add(1, Ordering::AcqRel) + 1;
                if attempt == 1 {
                    panic!("intentional panic for scheduler recovery test");
                }
                successes_task.fetch_add(1, Ordering::AcqRel);
            }
        });

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

        center.stop_task(task_id).await;
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
            .register_scheduled(
                "managed-manual",
                TaskSchedule::fixed(Duration::from_secs(60)),
                Some(on_event),
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
            .await
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
    async fn manual_trigger_resets_fixed_interval_from_now() {
        let center = TaskCenter::new();
        let runs = Arc::new(AtomicUsize::new(0));
        let triggers = Arc::new(Mutex::new(Vec::new()));
        let runs_task = runs.clone();
        let triggers_task = triggers.clone();
        let handle = center
            .register_scheduled(
                "manual-reset",
                TaskSchedule::fixed(Duration::from_secs(10)),
                None,
                move |context| {
                    let runs_task = runs_task.clone();
                    let triggers_task = triggers_task.clone();
                    async move {
                        runs_task.fetch_add(1, Ordering::Relaxed);
                        triggers_task.lock().unwrap().push(context.trigger);
                    }
                },
            )
            .await
            .unwrap();

        advance_and_flush(Duration::from_secs(5)).await;
        assert_eq!(handle.trigger().await, TriggerOutcome::Started);
        wait_until("manual run", || runs.load(Ordering::Relaxed) == 1).await;

        advance_and_flush(Duration::from_secs(5)).await;
        assert_eq!(
            runs.load(Ordering::Relaxed),
            1,
            "the original interval deadline must become stale"
        );

        advance_and_flush(Duration::from_secs(5)).await;
        wait_until("rescheduled fixed run", || {
            runs.load(Ordering::Relaxed) == 2
        })
        .await;
        assert_eq!(
            triggers.lock().unwrap().as_slice(),
            &[TaskTriggerKind::Manual, TaskTriggerKind::Scheduled]
        );
        handle.stop().await;
    }

    #[cfg(feature = "plugin-cron")]
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
        assert!(!utc.resets_after_manual_run());
    }

    #[cfg(feature = "plugin-cron")]
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

    #[cfg(feature = "plugin-cron")]
    #[tokio::test]
    async fn manual_trigger_preserves_existing_cron_deadline() {
        let schedule = TaskSchedule::cron("0 * * * *", "UTC").unwrap();
        let clock = TaskClock::new();
        let now = clock.now();
        let next_run = schedule.next_after(now, now).unwrap();
        let task: TaskFn = Arc::new(|_| Box::pin(async {}));
        let mut tasks = HashMap::from([(
            1,
            ScheduledTask {
                name: "calendar".to_string(),
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
        let (reply, response) = oneshot::channel();

        handle_trigger(TriggerCommand { id: 1, reply }, &mut tasks, &mut deadlines).await;

        assert_eq!(response.await.unwrap(), TriggerOutcome::Started);
        assert_eq!(tasks[&1].next_run, Some(deadline));
        stop_running_task(tasks.get_mut(&1).unwrap()).await;
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
                .register_scheduled(
                    "runtime-replacement",
                    TaskSchedule::fixed(Duration::from_secs(60)),
                    None,
                    move |_| {
                        let runs_task = runs_task.clone();
                        async move {
                            runs_task.fetch_add(1, Ordering::Relaxed);
                        }
                    },
                )
                .await
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
