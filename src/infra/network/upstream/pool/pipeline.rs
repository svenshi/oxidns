// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fmt::Debug;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU16, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use arc_swap::ArcSwap;
use async_trait::async_trait;
use tokio::sync::Notify;
use tracing::{debug, warn};

use crate::infra::clock::AppClock;
use crate::infra::error::Result;
use crate::infra::network::upstream::pool::{
    Connection, ConnectionBuilder, ConnectionPool, DeadlineOutcome, ManagedMaintenanceTask,
    QueryDeadline, QueryTimeoutPolicy, start_maintenance,
};
use crate::infra::task as task_center;
use crate::proto::Message;

const POOL_RETRY_BACKOFF: Duration = Duration::from_millis(10);

const SLOT_ACTIVE: u8 = 0;
const SLOT_RETIRING: u8 = 1;
const SLOT_CLOSED: u8 = 2;

#[derive(Debug)]
pub struct PipelinePool<C: Connection> {
    /// Round-robin index for load balancing across slots.
    index: AtomicUsize,
    /// List of connection slots (lock-free with ArcSwap).
    slots: ArcSwap<Vec<Arc<PipelineSlot<C>>>>,
    /// Connections currently being built and reserved against max_size.
    reserved_slots: AtomicUsize,
    /// Maximum number of connections allowed.
    max_size: usize,
    /// Minimum number of connections to maintain.
    min_size: usize,
    /// Maximum number of concurrent queries per connection.
    max_load: u16,
    /// Maximum allowed idle time before a connection is dropped.
    max_idle: Duration,
    /// Factory to create new connections.
    connection_builder: Box<dyn ConnectionBuilder<C>>,
    /// Per-query timeout policy for acquired slots.
    timeout_policy: QueryTimeoutPolicy,
    /// Timeout used only by background prefill/maintenance expansion.
    connect_timeout: Duration,
    /// Monotonic connection id source.
    next_id: AtomicU16,
    /// Notify waiters when a slot is released, inserted, retired, or closed.
    release_notified: Notify,
    /// Background maintenance task registered in task center.
    maintenance_task_id: Mutex<Option<u64>>,
}

#[async_trait]
impl<C: Connection> ConnectionPool<C> for PipelinePool<C> {
    async fn query(&self, request: Message, deadline: QueryDeadline) -> Result<Message> {
        let lease = self.acquire(deadline).await?;
        let result = deadline
            .run(lease.connection().query(request, deadline))
            .await;

        match result {
            DeadlineOutcome::Completed(result) => {
                if !lease.connection().available() {
                    lease.close();
                }
                result
            }
            DeadlineOutcome::Expired => {
                match self.timeout_policy {
                    QueryTimeoutPolicy::Reuse => {}
                    QueryTimeoutPolicy::Retire => lease.retire(),
                    QueryTimeoutPolicy::Close => lease.close(),
                }
                Err(deadline.timeout_error())
            }
        }
    }

    async fn maintain(&self) {
        let now = AppClock::elapsed_millis();
        let slots = self.slots.load();
        if slots.is_empty() {
            drop(slots);
            if self.min_size > 0 {
                let _ = self.expand(QueryDeadline::new(self.connect_timeout)).await;
            }
            return;
        }

        let mut keep = Vec::with_capacity(slots.len());
        let mut idle_candidates = Vec::new();
        let mut close_after_swap = Vec::new();

        for slot in slots.iter() {
            let state = slot.state();
            let inflight = slot.inflight();
            if state == SLOT_ACTIVE && slot.connection().available() {
                let idle = now.saturating_sub(slot.connection().last_used());
                if inflight == 0 && idle >= self.max_idle.as_millis() as u64 {
                    idle_candidates.push(slot.clone());
                } else {
                    keep.push(slot.clone());
                }
            } else if state == SLOT_RETIRING && inflight > 0 {
                keep.push(slot.clone());
            } else if inflight > 0 {
                slot.close();
                keep.push(slot.clone());
            } else {
                slot.close();
                close_after_swap.push(slot.clone());
            }
        }

        while keep.len() < self.min_size {
            let Some(slot) = idle_candidates.pop() else {
                break;
            };
            keep.push(slot);
        }
        for slot in idle_candidates {
            if slot.close_if_idle() {
                close_after_swap.push(slot);
            } else {
                keep.push(slot);
            }
        }

        let new_len = keep.len();
        if !Arc::ptr_eq(&slots, &self.slots.compare_and_swap(&slots, Arc::new(keep))) {
            for slot in close_after_swap {
                slot.close();
            }
            self.release_notified.notify_waiters();
            return;
        }

        for slot in &close_after_swap {
            slot.close();
        }

        if !close_after_swap.is_empty() {
            debug!(
                "Pipeline pool maintenance: removed {} slots, {} active",
                close_after_swap.len(),
                new_len
            );
            self.release_notified.notify_waiters();
        }

        if new_len < self.min_size {
            let _ = self.expand(QueryDeadline::new(self.connect_timeout)).await;
        }
    }

    #[cfg(test)]
    fn configured_min_size(&self) -> usize {
        self.min_size
    }
}

impl<C: Connection> PipelinePool<C> {
    pub fn new(
        min_size: usize,
        max_size: usize,
        max_load: u16,
        idle_time: Duration,
        connection_builder: Box<dyn ConnectionBuilder<C>>,
        timeout_policy: QueryTimeoutPolicy,
        connect_timeout: Duration,
    ) -> Arc<PipelinePool<C>> {
        let pool = Arc::new(Self {
            index: AtomicUsize::new(0),
            slots: ArcSwap::from_pointee(Vec::new()),
            reserved_slots: AtomicUsize::new(0),
            max_size,
            min_size,
            max_load: max_load.max(1),
            max_idle: idle_time,
            connection_builder,
            timeout_policy,
            connect_timeout,
            next_id: AtomicU16::new(1),
            release_notified: Notify::new(),
            maintenance_task_id: Mutex::new(None),
        });
        start_maintenance(&pool);
        if min_size > 0 {
            let arc = pool.clone();
            tokio::spawn(async move {
                if let Err(e) = arc.expand(QueryDeadline::new(arc.connect_timeout)).await {
                    warn!("Failed to prefill PipelinePool: {:?}", e);
                }
            });
        }
        pool
    }

    async fn acquire(&self, deadline: QueryDeadline) -> Result<PipelineLease<'_, C>> {
        loop {
            if let Some(slot) = self.try_acquire_existing() {
                return Ok(PipelineLease::new(slot, &self.release_notified));
            }

            if let Some(reservation) = self.try_reserve_slot() {
                match self.expand_one(reservation, deadline).await {
                    Ok(Some(slot)) => {
                        if slot.try_acquire(self.max_load) {
                            return Ok(PipelineLease::new(slot, &self.release_notified));
                        }
                        self.release_notified.notify_waiters();
                    }
                    Ok(None) => {
                        self.wait_backoff(deadline).await?;
                    }
                    Err(e) => {
                        if deadline.remaining().is_none() {
                            return Err(deadline.timeout_error());
                        }
                        debug!("Failed to create pipeline-pool connection: {:?}", e);
                        self.wait_backoff(deadline).await?;
                    }
                }
            } else {
                // Pool is saturated. Register as a waiter *before* the final
                // re-check so a slot released between the checks above and the
                // await is not lost: `enable()` claims any stored `notify_one`
                // permit, and once registered we are guaranteed to observe a
                // later `notify_waiters` from a close/retire/expand event.
                let notified = self.release_notified.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                if let Some(slot) = self.try_acquire_existing() {
                    return Ok(PipelineLease::new(slot, &self.release_notified));
                }
                if let Some(reservation) = self.try_reserve_slot() {
                    match self.expand_one(reservation, deadline).await {
                        Ok(Some(slot)) => {
                            if slot.try_acquire(self.max_load) {
                                return Ok(PipelineLease::new(slot, &self.release_notified));
                            }
                            self.release_notified.notify_waiters();
                        }
                        Ok(None) => {
                            self.wait_backoff(deadline).await?;
                            continue;
                        }
                        Err(e) => {
                            if deadline.remaining().is_none() {
                                return Err(deadline.timeout_error());
                            }
                            debug!("Failed to create pipeline-pool connection: {:?}", e);
                            self.wait_backoff(deadline).await?;
                            continue;
                        }
                    }
                }
                match deadline.run(notified.as_mut()).await {
                    DeadlineOutcome::Completed(()) => {}
                    DeadlineOutcome::Expired => return Err(deadline.timeout_error()),
                }
            }
        }
    }

    fn try_acquire_existing(&self) -> Option<Arc<PipelineSlot<C>>> {
        let slots = self.slots.load();
        let len = slots.len();
        if len == 0 {
            return None;
        }

        let start_idx = self.index.fetch_add(1, Ordering::Relaxed) % len;
        for offset in 0..len {
            let idx = (start_idx + offset) % len;
            let slot = &slots[idx];
            if slot.try_acquire(self.max_load) {
                return Some(slot.clone());
            }
        }

        None
    }

    async fn expand(&self, deadline: QueryDeadline) -> Result<()> {
        let current_len = self.slots.load().len();
        if current_len >= self.max_size {
            return Ok(());
        }

        let target = if current_len >= self.min_size {
            1
        } else {
            self.min_size - current_len
        };
        let want = target.min(self.max_size - current_len);

        for _ in 0..want {
            let Some(reservation) = self.try_reserve_slot() else {
                break;
            };
            self.expand_one(reservation, deadline).await?;
        }

        Ok(())
    }

    async fn expand_one(
        &self,
        reservation: SlotReservation<'_, C>,
        deadline: QueryDeadline,
    ) -> Result<Option<Arc<PipelineSlot<C>>>> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        match deadline
            .run(self.connection_builder.create_connection(id, deadline))
            .await
        {
            DeadlineOutcome::Completed(Ok(conn)) => {
                let slot = Arc::new(PipelineSlot::new(conn));
                if self.insert_slot(slot.clone()) {
                    reservation.commit();
                    debug!(
                        "Pipeline pool expanded: total={}/{}",
                        self.slots.load().len(),
                        self.max_size
                    );
                    self.release_notified.notify_waiters();
                    Ok(Some(slot))
                } else {
                    slot.close();
                    Ok(None)
                }
            }
            DeadlineOutcome::Completed(Err(e)) => Err(e),
            DeadlineOutcome::Expired => Err(deadline.timeout_error()),
        }
    }

    fn insert_slot(&self, slot: Arc<PipelineSlot<C>>) -> bool {
        let inserted = AtomicBool::new(false);
        self.slots.rcu(|old_slots| {
            let mut new_slots = Vec::with_capacity(old_slots.len() + 1);
            new_slots.extend(
                old_slots
                    .iter()
                    .filter(|slot| !slot.is_drained_unusable())
                    .cloned(),
            );
            let current_len = new_slots.len();
            if current_len >= self.max_size {
                inserted.store(false, Ordering::Relaxed);
                if current_len == old_slots.len() {
                    return old_slots.clone();
                }
                return Arc::new(new_slots);
            }

            new_slots.push(slot.clone());
            inserted.store(true, Ordering::Relaxed);
            Arc::new(new_slots)
        });
        inserted.load(Ordering::Relaxed)
    }

    fn try_reserve_slot(&self) -> Option<SlotReservation<'_, C>> {
        loop {
            let reserved = self.reserved_slots.load(Ordering::Acquire);
            let active = self.usable_or_inflight_slot_count();
            if active.saturating_add(reserved) >= self.max_size {
                return None;
            }
            match self.reserved_slots.compare_exchange_weak(
                reserved,
                reserved + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return Some(SlotReservation::new(self)),
                Err(_) => continue,
            }
        }
    }

    fn usable_or_inflight_slot_count(&self) -> usize {
        let slots = self.slots.load();
        let active = slots
            .iter()
            .filter(|slot| !slot.is_drained_unusable())
            .count();
        if active == slots.len() {
            return active;
        }

        let mut pruned = Vec::with_capacity(active);
        pruned.extend(
            slots
                .iter()
                .filter(|slot| !slot.is_drained_unusable())
                .cloned(),
        );
        let pruned_len = pruned.len();
        if Arc::ptr_eq(
            &slots,
            &self.slots.compare_and_swap(&slots, Arc::new(pruned)),
        ) {
            self.release_notified.notify_waiters();
            pruned_len
        } else {
            self.slots
                .load()
                .iter()
                .filter(|slot| !slot.is_drained_unusable())
                .count()
        }
    }

    async fn wait_backoff(&self, deadline: QueryDeadline) -> Result<()> {
        let Some(remaining) = deadline.remaining() else {
            return Err(deadline.timeout_error());
        };
        let delay = remaining.min(POOL_RETRY_BACKOFF);
        match deadline.run(tokio::time::sleep(delay)).await {
            DeadlineOutcome::Completed(()) => Ok(()),
            DeadlineOutcome::Expired => Err(deadline.timeout_error()),
        }
    }
}

#[derive(Debug)]
struct PipelineSlot<C: Connection> {
    conn: Arc<C>,
    inflight: AtomicU16,
    state: AtomicU8,
}

impl<C: Connection> PipelineSlot<C> {
    fn new(conn: Arc<C>) -> Self {
        Self {
            conn,
            inflight: AtomicU16::new(0),
            state: AtomicU8::new(SLOT_ACTIVE),
        }
    }

    fn connection(&self) -> &C {
        &self.conn
    }

    fn inflight(&self) -> u16 {
        self.inflight.load(Ordering::Acquire)
    }

    fn state(&self) -> u8 {
        self.state.load(Ordering::Acquire)
    }

    fn try_acquire(&self, max_load: u16) -> bool {
        if self.state() != SLOT_ACTIVE || !self.conn.available() {
            return false;
        }

        let mut current = self.inflight.load(Ordering::Acquire);
        loop {
            if current >= max_load {
                return false;
            }
            match self.inflight.compare_exchange_weak(
                current,
                current + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    if self.state() == SLOT_ACTIVE && self.conn.available() {
                        return true;
                    }
                    self.release_without_notify();
                    return false;
                }
                Err(next) => current = next,
            }
        }
    }

    fn release(&self, notify: &Notify) {
        let previous = self.inflight.fetch_sub(1, Ordering::AcqRel);
        debug_assert!(previous > 0, "pipeline slot inflight underflow");
        if previous == 1 && self.state() != SLOT_ACTIVE {
            self.close();
        }
        // Releasing one in-flight query frees exactly one unit of load on this
        // slot, so wake a single waiter instead of stampeding all of them.
        // `Notify` stores one permit if no waiter is currently parked, and the
        // freed capacity is in any case visible to other acquirers via
        // `try_acquire_existing`, so this cannot strand a waiter.
        notify.notify_one();
    }

    fn release_without_notify(&self) {
        let previous = self.inflight.fetch_sub(1, Ordering::AcqRel);
        debug_assert!(previous > 0, "pipeline slot inflight underflow");
        if previous == 1 && self.state() != SLOT_ACTIVE {
            self.close();
        }
    }

    fn retire(&self) {
        let _ = self.state.compare_exchange(
            SLOT_ACTIVE,
            SLOT_RETIRING,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        if self.inflight() == 0 {
            self.close();
        }
    }

    fn close(&self) {
        if self.state.swap(SLOT_CLOSED, Ordering::AcqRel) != SLOT_CLOSED {
            self.conn.close();
        }
    }

    fn close_if_idle(&self) -> bool {
        if self.inflight() != 0 {
            return false;
        }

        match self.state.compare_exchange(
            SLOT_ACTIVE,
            SLOT_CLOSED,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => {
                self.conn.close();
                true
            }
            Err(SLOT_RETIRING | SLOT_CLOSED) if self.inflight() == 0 => {
                self.close();
                true
            }
            Err(_) => false,
        }
    }

    fn is_drained_unusable(&self) -> bool {
        self.state() != SLOT_ACTIVE && self.inflight() == 0
    }
}

struct PipelineLease<'a, C: Connection> {
    slot: Arc<PipelineSlot<C>>,
    notify: &'a Notify,
}

impl<'a, C: Connection> PipelineLease<'a, C> {
    fn new(slot: Arc<PipelineSlot<C>>, notify: &'a Notify) -> Self {
        Self { slot, notify }
    }

    fn connection(&self) -> &C {
        self.slot.connection()
    }

    fn retire(&self) {
        self.slot.retire();
        self.notify.notify_waiters();
    }

    fn close(&self) {
        self.slot.close();
        self.notify.notify_waiters();
    }
}

impl<C: Connection> Drop for PipelineLease<'_, C> {
    fn drop(&mut self) {
        self.slot.release(self.notify);
    }
}

struct SlotReservation<'a, C: Connection> {
    pool: &'a PipelinePool<C>,
    active: bool,
}

impl<'a, C: Connection> SlotReservation<'a, C> {
    fn new(pool: &'a PipelinePool<C>) -> Self {
        Self { pool, active: true }
    }

    fn commit(mut self) {
        self.active = false;
        self.pool.reserved_slots.fetch_sub(1, Ordering::Release);
    }
}

impl<C: Connection> Drop for SlotReservation<'_, C> {
    fn drop(&mut self) {
        if self.active {
            self.pool.reserved_slots.fetch_sub(1, Ordering::Release);
            self.pool.release_notified.notify_waiters();
        }
    }
}

impl<C: Connection> ManagedMaintenanceTask for PipelinePool<C> {
    fn maintenance_task_id(&self) -> &Mutex<Option<u64>> {
        &self.maintenance_task_id
    }

    fn maintenance_task_name(&self) -> String {
        "upstream_pipeline_pool:maintenance".to_string()
    }
}

impl<C: Connection> Drop for PipelinePool<C> {
    fn drop(&mut self) {
        let task_id = self
            .maintenance_task_id
            .lock()
            .ok()
            .and_then(|mut guard| guard.take());
        if let Some(task_id) = task_id {
            task_center::stop_task_detached(task_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, AtomicU64};

    use async_trait::async_trait;

    use super::*;
    use crate::infra::error::{DnsError, Result};

    #[derive(Debug)]
    struct MockConnection {
        available: AtomicBool,
        using_count: AtomicU16,
        last_used: AtomicU64,
        close_calls: AtomicUsize,
        query_delay: Duration,
    }

    impl MockConnection {
        fn new(available: bool, using_count: u16, last_used: u64) -> Self {
            Self {
                available: AtomicBool::new(available),
                using_count: AtomicU16::new(using_count),
                last_used: AtomicU64::new(last_used),
                close_calls: AtomicUsize::new(0),
                query_delay: Duration::ZERO,
            }
        }

        fn with_query_delay(mut self, query_delay: Duration) -> Self {
            self.query_delay = query_delay;
            self
        }

        fn close_calls(&self) -> usize {
            self.close_calls.load(Ordering::Relaxed)
        }
    }

    #[async_trait]
    impl Connection for MockConnection {
        fn close(&self) {
            self.close_calls.fetch_add(1, Ordering::Relaxed);
            self.available.store(false, Ordering::Relaxed);
        }

        async fn query(&self, request: Message, _deadline: QueryDeadline) -> Result<Message> {
            self.using_count.fetch_add(1, Ordering::Relaxed);
            if !self.query_delay.is_zero() {
                tokio::time::sleep(self.query_delay).await;
            }
            self.using_count.fetch_sub(1, Ordering::Relaxed);
            Ok(request)
        }

        fn using_count(&self) -> u16 {
            self.using_count.load(Ordering::Relaxed)
        }

        fn available(&self) -> bool {
            self.available.load(Ordering::Relaxed)
        }

        fn last_used(&self) -> u64 {
            self.last_used.load(Ordering::Relaxed)
        }
    }

    #[derive(Debug)]
    struct MockBuilder {
        planned: Mutex<VecDeque<Result<Arc<MockConnection>>>>,
    }

    impl MockBuilder {
        fn new(planned: Vec<Result<Arc<MockConnection>>>) -> Self {
            Self {
                planned: Mutex::new(planned.into()),
            }
        }
    }

    #[async_trait]
    impl ConnectionBuilder<MockConnection> for MockBuilder {
        async fn create_connection(
            &self,
            _conn_id: u16,
            _deadline: QueryDeadline,
        ) -> Result<Arc<MockConnection>> {
            self.planned
                .lock()
                .expect("builder plan lock should not be poisoned")
                .pop_front()
                .unwrap_or_else(|| Err(DnsError::runtime("no planned connection")))
        }
    }

    fn make_pool(
        min_size: usize,
        max_size: usize,
        max_load: u16,
        idle_secs: u64,
        builder: MockBuilder,
        initial_connections: Vec<Arc<MockConnection>>,
    ) -> PipelinePool<MockConnection> {
        AppClock::start();
        let slots = initial_connections
            .into_iter()
            .map(|conn| Arc::new(PipelineSlot::new(conn)))
            .collect();
        PipelinePool {
            index: AtomicUsize::new(0),
            slots: ArcSwap::from_pointee(slots),
            reserved_slots: AtomicUsize::new(0),
            max_size,
            min_size,
            max_load: max_load.max(1),
            max_idle: Duration::from_secs(idle_secs),
            connection_builder: Box::new(builder),
            timeout_policy: QueryTimeoutPolicy::Retire,
            connect_timeout: Duration::from_secs(5),
            next_id: AtomicU16::new(1),
            release_notified: Notify::new(),
            maintenance_task_id: Mutex::new(None),
        }
    }

    #[tokio::test]
    async fn test_acquire_uses_round_robin_across_connections() {
        let first = Arc::new(MockConnection::new(true, 0, 0));
        let second = Arc::new(MockConnection::new(true, 0, 0));
        let pool = make_pool(
            0,
            2,
            4,
            10,
            MockBuilder::new(vec![]),
            vec![first.clone(), second.clone()],
        );

        let first_lease = pool
            .acquire(QueryDeadline::new(Duration::from_secs(1)))
            .await
            .expect("first acquire should succeed");
        let second_lease = pool
            .acquire(QueryDeadline::new(Duration::from_secs(1)))
            .await
            .expect("second acquire should succeed");

        assert!(Arc::ptr_eq(&first_lease.slot.conn, &first));
        assert!(Arc::ptr_eq(&second_lease.slot.conn, &second));
    }

    #[tokio::test]
    async fn test_acquire_expands_when_pool_is_empty() {
        let created = Arc::new(MockConnection::new(true, 0, 0));
        let pool = make_pool(
            0,
            1,
            4,
            10,
            MockBuilder::new(vec![Ok(created.clone())]),
            vec![],
        );

        let lease = pool
            .acquire(QueryDeadline::new(Duration::from_millis(100)))
            .await
            .expect("acquire should expand an empty pool");

        assert!(Arc::ptr_eq(&lease.slot.conn, &created));
        assert_eq!(pool.slots.load().len(), 1);
    }

    #[tokio::test]
    async fn test_acquire_does_not_oversell_max_load() {
        let conn = Arc::new(MockConnection::new(true, 0, 0));
        let pool = make_pool(0, 1, 2, 10, MockBuilder::new(vec![]), vec![conn.clone()]);

        let lease_a = pool
            .acquire(QueryDeadline::new(Duration::from_secs(1)))
            .await
            .expect("first acquire should succeed");
        let lease_b = pool
            .acquire(QueryDeadline::new(Duration::from_secs(1)))
            .await
            .expect("second acquire should succeed");
        let blocked = tokio::time::timeout(
            Duration::from_millis(20),
            pool.acquire(QueryDeadline::new(Duration::from_secs(1))),
        )
        .await;

        assert!(blocked.is_err());
        assert_eq!(lease_a.slot.inflight(), 2);
        drop(lease_b);
    }

    #[tokio::test]
    async fn test_acquire_waits_until_slot_release() {
        let conn = Arc::new(MockConnection::new(true, 0, 0));
        let pool = Arc::new(make_pool(
            0,
            1,
            1,
            10,
            MockBuilder::new(vec![]),
            vec![conn.clone()],
        ));
        let lease = pool
            .acquire(QueryDeadline::new(Duration::from_secs(1)))
            .await
            .expect("first acquire should succeed");

        let waiting_pool = pool.clone();
        let expected_conn = conn.clone();
        let waiter = tokio::spawn(async move {
            let next = waiting_pool
                .acquire(QueryDeadline::new(Duration::from_secs(1)))
                .await?;
            Ok::<_, DnsError>(Arc::ptr_eq(&next.slot.conn, &expected_conn))
        });
        tokio::time::sleep(Duration::from_millis(10)).await;
        drop(lease);

        let matched = tokio::time::timeout(Duration::from_secs(1), waiter)
            .await
            .expect("waiter should be woken")
            .expect("join should succeed")
            .expect("acquire should succeed");

        assert!(matched);
    }

    #[tokio::test]
    async fn test_acquire_replaces_drained_slot_after_capacity_is_freed() {
        AppClock::start();
        let stale = Arc::new(MockConnection::new(true, 0, AppClock::elapsed_millis()));
        let replacement = Arc::new(MockConnection::new(true, 0, AppClock::elapsed_millis()));
        let pool = Arc::new(make_pool(
            0,
            1,
            1,
            10,
            MockBuilder::new(vec![Ok(replacement.clone())]),
            vec![stale.clone()],
        ));
        let lease = pool
            .acquire(QueryDeadline::new(Duration::from_secs(1)))
            .await
            .expect("first acquire should saturate the only slot");

        let waiting_pool = pool.clone();
        let expected_replacement = replacement.clone();
        let waiter = tokio::spawn(async move {
            let next = waiting_pool
                .acquire(QueryDeadline::new(Duration::from_secs(1)))
                .await?;
            Ok::<_, DnsError>(Arc::ptr_eq(&next.slot.conn, &expected_replacement))
        });
        tokio::time::sleep(Duration::from_millis(10)).await;

        lease.close();
        drop(lease);

        let matched = tokio::time::timeout(Duration::from_secs(1), waiter)
            .await
            .expect("waiter should not remain parked after capacity is freed")
            .expect("join should succeed")
            .expect("acquire should create a replacement slot");

        assert!(matched);
        assert_eq!(stale.close_calls(), 1);
        assert_eq!(pool.slots.load().len(), 1);
    }

    #[tokio::test]
    async fn test_query_timeout_retires_slot_without_closing_active_peer() {
        AppClock::start();
        let conn = Arc::new(
            MockConnection::new(true, 0, AppClock::elapsed_millis())
                .with_query_delay(Duration::from_secs(60)),
        );
        let pool = make_pool(0, 1, 2, 10, MockBuilder::new(vec![]), vec![conn.clone()]);

        let fast_lease = pool
            .acquire(QueryDeadline::new(Duration::from_secs(1)))
            .await
            .expect("acquire should succeed");
        let result = pool
            .query(
                Message::new(),
                QueryDeadline::new(Duration::from_millis(10)),
            )
            .await;

        assert!(result.is_err());
        assert_eq!(conn.close_calls(), 0);
        assert_eq!(pool.slots.load()[0].state(), SLOT_RETIRING);
        drop(fast_lease);
        assert_eq!(conn.close_calls(), 1);
    }

    #[tokio::test]
    async fn test_maintain_removes_retired_drained_slot() {
        AppClock::start();
        let conn = Arc::new(MockConnection::new(true, 0, AppClock::elapsed_millis()));
        let slot = Arc::new(PipelineSlot::new(conn.clone()));
        slot.retire();
        let pool = make_pool(0, 1, 4, 10, MockBuilder::new(vec![]), vec![]);
        pool.slots.store(Arc::new(vec![slot]));

        pool.maintain().await;

        assert!(pool.slots.load().is_empty());
        assert_eq!(conn.close_calls(), 1);
    }

    #[tokio::test]
    async fn test_acquire_replaces_drained_unusable_slot_at_capacity() {
        AppClock::start();
        let stale = Arc::new(MockConnection::new(true, 0, AppClock::elapsed_millis()));
        let replacement = Arc::new(MockConnection::new(true, 0, AppClock::elapsed_millis()));
        let pool = make_pool(
            0,
            1,
            4,
            10,
            MockBuilder::new(vec![Ok(replacement.clone())]),
            vec![stale.clone()],
        );
        pool.slots.load()[0].close();

        let lease = pool
            .acquire(QueryDeadline::new(Duration::from_millis(100)))
            .await
            .expect("closed slot should be pruned before reserving a replacement");

        assert!(Arc::ptr_eq(&lease.slot.conn, &replacement));
        assert_eq!(pool.slots.load().len(), 1);
        assert_eq!(stale.close_calls(), 1);
    }

    #[tokio::test]
    async fn test_maintain_keeps_retiring_slot_until_inflight_drains() {
        AppClock::start();
        let conn = Arc::new(MockConnection::new(true, 0, AppClock::elapsed_millis()));
        let pool = make_pool(0, 1, 4, 10, MockBuilder::new(vec![]), vec![conn.clone()]);
        let lease = pool
            .acquire(QueryDeadline::new(Duration::from_secs(1)))
            .await
            .expect("acquire should succeed");
        lease.retire();

        pool.maintain().await;

        assert_eq!(pool.slots.load().len(), 1);
        assert_eq!(conn.close_calls(), 0);

        drop(lease);
        assert_eq!(conn.close_calls(), 1);
    }

    #[tokio::test]
    async fn test_maintain_drops_idle_and_invalid_connections() {
        AppClock::start();
        let idle = Arc::new(MockConnection::new(true, 0, 0));
        let invalid = Arc::new(MockConnection::new(false, 0, 0));
        let pool = make_pool(
            0,
            4,
            4,
            0,
            MockBuilder::new(vec![]),
            vec![idle.clone(), invalid.clone()],
        );

        pool.maintain().await;

        assert_eq!(idle.close_calls(), 1);
        assert_eq!(invalid.close_calls(), 1);
        assert!(pool.slots.load().is_empty());
    }

    #[tokio::test]
    async fn test_maintain_marks_removed_idle_slot_closed() {
        AppClock::start();
        let conn = Arc::new(MockConnection::new(true, 0, 0));
        let pool = make_pool(0, 1, 4, 0, MockBuilder::new(vec![]), vec![conn.clone()]);
        let slot = pool.slots.load()[0].clone();

        pool.maintain().await;

        assert_eq!(slot.state(), SLOT_CLOSED);
        assert!(!slot.try_acquire(1));
        assert_eq!(conn.close_calls(), 1);
        assert!(pool.slots.load().is_empty());
    }

    #[tokio::test]
    async fn test_maintain_reuses_idle_connection_to_preserve_min_size() {
        AppClock::start();
        let conn = Arc::new(MockConnection::new(true, 0, 0));
        let pool = make_pool(1, 1, 4, 0, MockBuilder::new(vec![]), vec![conn.clone()]);

        pool.maintain().await;

        assert_eq!(conn.close_calls(), 0);
        assert_eq!(pool.slots.load().len(), 1);
    }

    #[tokio::test]
    async fn test_maintain_keeps_idle_connection_with_inflight_queries() {
        AppClock::start();
        let conn = Arc::new(MockConnection::new(true, 0, 0));
        let pool = make_pool(0, 1, 4, 0, MockBuilder::new(vec![]), vec![conn.clone()]);
        let lease = pool
            .acquire(QueryDeadline::new(Duration::from_secs(1)))
            .await
            .expect("acquire should succeed");

        pool.maintain().await;

        assert_eq!(conn.close_calls(), 0);
        assert_eq!(pool.slots.load().len(), 1);
        drop(lease);
    }
}
