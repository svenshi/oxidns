// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fmt::Debug;
use std::sync::atomic::{AtomicU16, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use crossbeam_queue::ArrayQueue;
use tokio::sync::Notify;
use tracing::{debug, info, warn};

use crate::infra::clock::AppClock;
use crate::infra::error::Result;
use crate::infra::network::upstream::pool::{
    Connection, ConnectionBuilder, ConnectionPool, DeadlineOutcome, ManagedMaintenanceTask,
    QueryDeadline, QueryTimeoutPolicy, start_maintenance,
};
use crate::infra::task as task_center;
use crate::proto::Message;

const POOL_RETRY_BACKOFF: Duration = Duration::from_millis(10);

#[inline]
fn close_conns<C: Connection>(conns: &[Arc<C>]) {
    for conn in conns {
        conn.close();
    }
}

/// A reusable connection pool implementation
/// - Keeps a minimum number of active connections (`min_size`)
/// - Can expand up to `max_size` when needed
/// - Reuses idle connections, and drops those idle beyond `max_idle`
/// - Thread-safe, designed for async DNS request handling
#[derive(Debug)]
pub struct ReusePool<C: Connection> {
    /// Queue holding idle connections
    connections: ArrayQueue<Arc<C>>,
    /// Number of active connections in use or queued
    active_count: AtomicUsize,
    /// Maximum number of connections allowed
    max_size: usize,
    /// Minimum number of connections to keep alive
    min_size: usize,
    /// Maximum allowed idle duration before dropping a connection
    max_idle: Duration,
    /// Factory to create new connections
    connection_builder: Box<dyn ConnectionBuilder<C>>,
    /// Per-query timeout policy for borrowed connections.
    timeout_policy: QueryTimeoutPolicy,
    /// Timeout used only by background prefill/maintenance expansion.
    connect_timeout: Duration,
    /// Monotonic increasing connection id
    next_id: AtomicU16,
    /// Notify waiting threads when a connection becomes available
    release_notified: Notify,
    /// Background maintenance task registered in task center.
    maintenance_task_id: Mutex<Option<u64>>,
}

#[async_trait]
impl<C: Connection> ConnectionPool<C> for ReusePool<C> {
    /// Obtain a connection, execute query, and release it back to the pool
    async fn query(&self, request: Message, deadline: QueryDeadline) -> Result<Message> {
        let borrowed = self.get(deadline).await?;
        debug!(
            "Got connection from pool, using_count={}",
            borrowed.connection().using_count()
        );
        let result = deadline
            .run(borrowed.connection().query(request, deadline))
            .await;

        match result {
            DeadlineOutcome::Completed(result) => {
                if borrowed.connection().available() {
                    borrowed.release();
                } else {
                    borrowed.close();
                }
                result
            }
            DeadlineOutcome::Expired => {
                match self.timeout_policy {
                    QueryTimeoutPolicy::Reuse if borrowed.connection().available() => {
                        borrowed.release();
                    }
                    QueryTimeoutPolicy::Reuse
                    | QueryTimeoutPolicy::Retire
                    | QueryTimeoutPolicy::Close => {
                        borrowed.close();
                    }
                }
                Err(deadline.timeout_error())
            }
        }
    }

    /// Periodic pool maintenance task
    /// - Removes idle/invalid connections
    /// - Ensures minimum connection count
    async fn maintain(&self) {
        let now = AppClock::elapsed_millis();
        let mut drop_vec = Vec::new();
        let mut invalid_vec = Vec::new();

        let check_count = self.connections.len();
        if check_count == 0 {
            if self.active_count.load(Ordering::Relaxed) < self.min_size {
                debug!("Reuse pool expanding to maintain minimum size");
                let _ = self.expand(QueryDeadline::new(self.connect_timeout)).await;
            }
            return;
        }

        for _ in 0..check_count {
            if let Some(conn) = self.connections.pop() {
                if conn.available() {
                    let idle = now - conn.last_used();
                    if idle < self.max_idle.as_millis() as u64 || conn.using_count() > 0 {
                        // still valid
                        if let Err(conn) = self.connections.push(conn) {
                            drop_vec.push(conn);
                            self.active_count.fetch_sub(1, Ordering::Relaxed);
                        }
                    } else {
                        // idle timeout
                        drop_vec.push(conn);
                        self.active_count.fetch_sub(1, Ordering::Relaxed);
                    }
                } else {
                    debug!("Dropping invalid connection");
                    invalid_vec.push(conn);
                    self.active_count.fetch_sub(1, Ordering::Relaxed);
                }
            } else {
                break;
            }
        }

        // Maintain minimum connection count
        while self.active_count.load(Ordering::Relaxed) < self.min_size {
            if !drop_vec.is_empty() {
                if let Err(conn) = self.connections.push(drop_vec.pop().unwrap()) {
                    drop_vec.push(conn);
                    break;
                } else {
                    self.active_count.fetch_add(1, Ordering::Relaxed);
                }
            } else {
                break;
            }
        }

        // Close dropped/invalid connections
        close_conns(&drop_vec);
        close_conns(&invalid_vec);

        // Log maintenance results if significant
        if !drop_vec.is_empty() || !invalid_vec.is_empty() {
            debug!(
                "Reuse pool maintenance: dropped {} idle, {} invalid, {} active",
                drop_vec.len(),
                invalid_vec.len(),
                self.active_count.load(Ordering::Relaxed)
            );
        }

        // Expand if below min_size
        if self.active_count.load(Ordering::Relaxed) < self.min_size {
            debug!("Reuse pool expanding to maintain minimum size");
            let _ = self.expand(QueryDeadline::new(self.connect_timeout)).await;
        }
    }

    #[cfg(test)]
    fn configured_min_size(&self) -> usize {
        self.min_size
    }
}

impl<C: Connection> ReusePool<C> {
    /// Create a new reusable connection pool
    pub fn new(
        min_size: usize,
        max_size: usize,
        idle_time: Duration,
        connection_builder: Box<dyn ConnectionBuilder<C>>,
        timeout_policy: QueryTimeoutPolicy,
        connect_timeout: Duration,
    ) -> Arc<ReusePool<C>> {
        info!(
            "Creating ReusePool (min_size={}, max_size={})",
            min_size, max_size
        );

        let pool = Arc::new(Self {
            connections: ArrayQueue::new(max_size),
            min_size,
            max_size,
            connection_builder,
            timeout_policy,
            connect_timeout,
            max_idle: idle_time,
            active_count: AtomicUsize::new(0),
            next_id: AtomicU16::new(1),
            release_notified: Notify::new(),
            maintenance_task_id: Mutex::new(None),
        });

        start_maintenance(&pool);

        if min_size > 0 {
            let arc = pool.clone();
            tokio::spawn(async move {
                if let Err(e) = arc.expand(QueryDeadline::new(arc.connect_timeout)).await {
                    warn!("Failed to prefill ReusePool: {:?}", e);
                }
            });
        }

        pool
    }

    /// Borrow a connection from the pool or create a new one if needed
    async fn get(&self, deadline: QueryDeadline) -> Result<BorrowedConnection<'_, C>> {
        loop {
            if let Some(conn) = self.connections.pop() {
                if conn.available() {
                    debug!("Reusing existing connection");
                    return Ok(BorrowedConnection::new(self, conn));
                } else {
                    warn!("Detected unavailable connection, closing it");
                    self.close_active_connection(conn);
                }
            }

            if let Some(reservation) = self.try_reserve_active() {
                match self.create_reserved_connection(reservation, deadline).await {
                    Ok(conn) => return Ok(BorrowedConnection::new(self, conn)),
                    Err(e) => {
                        if deadline.remaining().is_none() {
                            return Err(deadline.timeout_error());
                        }
                        debug!("Failed to create reuse-pool connection: {:?}", e);
                        self.wait_backoff(deadline).await?;
                    }
                }
            } else {
                debug!("Pool is full, waiting for release...");
                // Register as a waiter *before* the final re-check so a
                // connection released between the check above and the await is
                // not lost: `enable()` claims any stored `notify_one` permit,
                // and once registered we will observe a later `notify_waiters`
                // from a close that frees capacity.
                let notified = self.release_notified.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                if let Some(conn) = self.connections.pop() {
                    if conn.available() {
                        debug!("Reusing existing connection");
                        return Ok(BorrowedConnection::new(self, conn));
                    }
                    warn!("Detected unavailable connection, closing it");
                    self.close_active_connection(conn);
                    continue;
                }
                if let Some(reservation) = self.try_reserve_active() {
                    match self.create_reserved_connection(reservation, deadline).await {
                        Ok(conn) => return Ok(BorrowedConnection::new(self, conn)),
                        Err(e) => {
                            if deadline.remaining().is_none() {
                                return Err(deadline.timeout_error());
                            }
                            debug!("Failed to create reuse-pool connection: {:?}", e);
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

    /// Return a connection back to the pool or close it if invalid
    fn release(&self, conn: Arc<C>) {
        if !conn.available() || self.connections.push(conn.clone()).is_err() {
            warn!("Releasing invalid or overflowed connection, closing it");
            self.close_active_connection(conn);
        } else {
            debug!("Connection released back to pool");
            self.release_notified.notify_one();
        }
    }

    fn close_active_connection(&self, conn: Arc<C>) {
        conn.close();
        self.active_count.fetch_sub(1, Ordering::Relaxed);
        self.release_notified.notify_waiters();
    }

    /// Expand pool by creating new connections up to desired size
    async fn expand(&self, deadline: QueryDeadline) -> Result<()> {
        let conns_len = self.active_count.load(Ordering::Relaxed);
        if conns_len >= self.max_size {
            debug!("Pool already at max capacity ({})", self.max_size);
            return Ok(());
        }

        let mut want = if conns_len >= self.min_size {
            1
        } else {
            self.min_size - conns_len
        };
        if conns_len + want > self.max_size {
            want = self.max_size - conns_len;
        }
        if want == 0 {
            return Ok(());
        }

        let mut created = Vec::with_capacity(want);
        for _ in 0..want {
            let Some(reservation) = self.try_reserve_active() else {
                break;
            };
            match self.create_reserved_connection(reservation, deadline).await {
                Ok(conn) => {
                    if self.connections.push(conn.clone()).is_ok() {
                        created.push(conn);
                        self.release_notified.notify_one();
                    } else {
                        debug!("Pool queue is full while expanding, closing new connection");
                        self.close_active_connection(conn);
                    }
                }
                Err(e) => {
                    debug!("Failed to create new connection: {:?}", e);
                    if deadline.remaining().is_none() {
                        return Err(deadline.timeout_error());
                    }
                }
            }
        }

        let created_len = created.len();

        if created_len > 0 {
            debug!(
                "Reuse pool expanded: +{} connections (total={}/{})",
                created_len,
                self.active_count.load(Ordering::Relaxed),
                self.max_size
            );
        }

        Ok(())
    }

    fn try_reserve_active(&self) -> Option<ActiveReservation<'_, C>> {
        let mut current = self.active_count.load(Ordering::Acquire);
        loop {
            if current >= self.max_size {
                return None;
            }
            match self.active_count.compare_exchange_weak(
                current,
                current + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return Some(ActiveReservation::new(self)),
                Err(next) => current = next,
            }
        }
    }

    async fn create_reserved_connection(
        &self,
        reservation: ActiveReservation<'_, C>,
        deadline: QueryDeadline,
    ) -> Result<Arc<C>> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        match deadline
            .run(self.connection_builder.create_connection(id, deadline))
            .await
        {
            DeadlineOutcome::Completed(Ok(conn)) => {
                reservation.commit();
                Ok(conn)
            }
            DeadlineOutcome::Completed(Err(e)) => Err(e),
            DeadlineOutcome::Expired => Err(deadline.timeout_error()),
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

struct ActiveReservation<'a, C: Connection> {
    pool: &'a ReusePool<C>,
    active: bool,
}

impl<'a, C: Connection> ActiveReservation<'a, C> {
    fn new(pool: &'a ReusePool<C>) -> Self {
        Self { pool, active: true }
    }

    fn commit(mut self) {
        self.active = false;
    }
}

impl<C: Connection> Drop for ActiveReservation<'_, C> {
    fn drop(&mut self) {
        if self.active {
            self.pool.active_count.fetch_sub(1, Ordering::Relaxed);
            self.pool.release_notified.notify_waiters();
        }
    }
}

struct BorrowedConnection<'a, C: Connection> {
    pool: &'a ReusePool<C>,
    conn: Option<Arc<C>>,
}

impl<'a, C: Connection> BorrowedConnection<'a, C> {
    fn new(pool: &'a ReusePool<C>, conn: Arc<C>) -> Self {
        Self {
            pool,
            conn: Some(conn),
        }
    }

    fn connection(&self) -> &C {
        self.conn
            .as_deref()
            .expect("borrowed connection should exist until released")
    }

    fn release(mut self) {
        if let Some(conn) = self.conn.take() {
            self.pool.release(conn);
        }
    }

    fn close(mut self) {
        if let Some(conn) = self.conn.take() {
            self.pool.close_active_connection(conn);
        }
    }
}

impl<C: Connection> Drop for BorrowedConnection<'_, C> {
    fn drop(&mut self) {
        if let Some(conn) = self.conn.take() {
            warn!("Borrowed reuse-pool connection dropped before release, closing it");
            self.pool.close_active_connection(conn);
        }
    }
}

impl<C: Connection> ManagedMaintenanceTask for ReusePool<C> {
    fn maintenance_task_id(&self) -> &Mutex<Option<u64>> {
        &self.maintenance_task_id
    }

    fn maintenance_task_name(&self) -> String {
        "upstream_reuse_pool:maintenance".to_string()
    }
}

impl<C: Connection> Drop for ReusePool<C> {
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

    use super::*;
    use crate::infra::error::{DnsError, Result};
    use crate::proto::Message;

    #[derive(Debug)]
    struct MockConnection {
        available: AtomicBool,
        using_count: AtomicU16,
        last_used: AtomicU64,
        close_calls: AtomicUsize,
        query_calls: AtomicUsize,
        query_delay: Duration,
    }

    impl MockConnection {
        fn new(available: bool, using_count: u16, last_used: u64) -> Self {
            Self {
                available: AtomicBool::new(available),
                using_count: AtomicU16::new(using_count),
                last_used: AtomicU64::new(last_used),
                close_calls: AtomicUsize::new(0),
                query_calls: AtomicUsize::new(0),
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

        fn query_calls(&self) -> usize {
            self.query_calls.load(Ordering::Relaxed)
        }
    }

    #[async_trait]
    impl Connection for MockConnection {
        fn close(&self) {
            self.close_calls.fetch_add(1, Ordering::Relaxed);
            self.available.store(false, Ordering::Relaxed);
        }

        async fn query(&self, request: Message, _deadline: QueryDeadline) -> Result<Message> {
            self.query_calls.fetch_add(1, Ordering::Relaxed);
            if !self.query_delay.is_zero() {
                tokio::time::sleep(self.query_delay).await;
            }
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
        idle_secs: u64,
        builder: MockBuilder,
    ) -> ReusePool<MockConnection> {
        AppClock::start();
        ReusePool {
            connections: ArrayQueue::new(max_size.max(1)),
            active_count: AtomicUsize::new(0),
            max_size,
            min_size,
            max_idle: Duration::from_secs(idle_secs),
            connection_builder: Box::new(builder),
            timeout_policy: QueryTimeoutPolicy::Close,
            connect_timeout: Duration::from_secs(5),
            next_id: AtomicU16::new(1),
            release_notified: Notify::new(),
            maintenance_task_id: Mutex::new(None),
        }
    }

    #[test]
    fn test_close_conns_closes_every_connection_once() {
        let first = Arc::new(MockConnection::new(true, 0, 0));
        let second = Arc::new(MockConnection::new(true, 0, 0));
        let conns = vec![first.clone(), second.clone()];

        close_conns(&conns);

        assert_eq!(first.close_calls(), 1);
        assert_eq!(second.close_calls(), 1);
    }

    #[tokio::test]
    async fn test_get_reuses_available_connection_from_queue() {
        let pool = make_pool(0, 2, 10, MockBuilder::new(vec![]));
        let conn = Arc::new(MockConnection::new(true, 0, 0));
        pool.connections
            .push(conn.clone())
            .expect("queue should accept connection");
        pool.active_count.store(1, Ordering::Relaxed);

        let selected = pool
            .get(QueryDeadline::new(Duration::from_secs(1)))
            .await
            .expect("get should reuse queued connection");

        assert!(Arc::ptr_eq(
            selected
                .conn
                .as_ref()
                .expect("borrowed connection should exist"),
            &conn
        ));
        assert_eq!(pool.active_count.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn test_get_closes_unavailable_connection_and_expands_replacement() {
        let replacement = Arc::new(MockConnection::new(true, 0, 0));
        let pool = make_pool(0, 2, 10, MockBuilder::new(vec![Ok(replacement.clone())]));
        let stale = Arc::new(MockConnection::new(false, 0, 0));
        pool.connections
            .push(stale.clone())
            .expect("queue should accept stale connection");
        pool.active_count.store(1, Ordering::Relaxed);

        let selected = pool
            .get(QueryDeadline::new(Duration::from_secs(1)))
            .await
            .expect("get should expand a replacement connection");

        assert!(Arc::ptr_eq(
            selected
                .conn
                .as_ref()
                .expect("borrowed connection should exist"),
            &replacement
        ));
        assert_eq!(stale.close_calls(), 1);
        assert_eq!(pool.active_count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_release_closes_unavailable_connection_instead_of_requeueing() {
        let pool = make_pool(0, 2, 10, MockBuilder::new(vec![]));
        let conn = Arc::new(MockConnection::new(false, 0, 0));
        pool.active_count.store(1, Ordering::Relaxed);

        pool.release(conn.clone());

        assert_eq!(conn.close_calls(), 1);
        assert_eq!(pool.connections.len(), 0);
        assert_eq!(pool.active_count.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn test_maintain_drops_idle_and_invalid_connections() {
        AppClock::start();
        let pool = make_pool(0, 4, 0, MockBuilder::new(vec![]));
        let idle = Arc::new(MockConnection::new(true, 0, 0));
        let invalid = Arc::new(MockConnection::new(false, 0, 0));
        pool.connections
            .push(idle.clone())
            .expect("queue should accept idle connection");
        pool.connections
            .push(invalid.clone())
            .expect("queue should accept invalid connection");
        pool.active_count.store(2, Ordering::Relaxed);

        pool.maintain().await;

        assert_eq!(idle.close_calls(), 1);
        assert_eq!(invalid.close_calls(), 1);
        assert_eq!(pool.connections.len(), 0);
        assert_eq!(pool.active_count.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn test_maintain_reuses_idle_connection_to_preserve_min_size() {
        AppClock::start();
        let pool = make_pool(1, 1, 0, MockBuilder::new(vec![]));
        let conn = Arc::new(MockConnection::new(true, 0, 0));
        pool.connections
            .push(conn.clone())
            .expect("queue should accept connection");
        pool.active_count.store(1, Ordering::Relaxed);

        pool.maintain().await;

        assert_eq!(conn.close_calls(), 0);
        assert_eq!(pool.connections.len(), 1);
        assert_eq!(pool.active_count.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn test_maintain_expands_empty_pool_to_preserve_min_size() {
        AppClock::start();
        let conn = Arc::new(MockConnection::new(true, 0, AppClock::elapsed_millis()));
        let pool = make_pool(1, 1, 10, MockBuilder::new(vec![Ok(conn.clone())]));

        pool.maintain().await;

        assert_eq!(pool.connections.len(), 1);
        assert_eq!(pool.active_count.load(Ordering::Relaxed), 1);
        let pooled = pool
            .connections
            .pop()
            .expect("maintenance should create a replacement connection");
        assert!(Arc::ptr_eq(&pooled, &conn));
    }

    #[tokio::test]
    async fn test_maintain_keeps_idle_connection_with_inflight_queries() {
        AppClock::start();
        let pool = make_pool(0, 1, 0, MockBuilder::new(vec![]));
        let conn = Arc::new(MockConnection::new(true, 1, 0));
        pool.connections
            .push(conn.clone())
            .expect("queue should accept connection");
        pool.active_count.store(1, Ordering::Relaxed);

        pool.maintain().await;

        assert_eq!(conn.close_calls(), 0);
        assert_eq!(pool.connections.len(), 1);
        assert_eq!(pool.active_count.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn test_query_releases_connection_back_to_pool_after_success() {
        let pool = make_pool(0, 1, 10, MockBuilder::new(vec![]));
        let conn = Arc::new(MockConnection::new(true, 0, 0));
        pool.connections
            .push(conn.clone())
            .expect("queue should accept connection");
        pool.active_count.store(1, Ordering::Relaxed);
        let mut request = Message::new();
        request.set_id(21);

        let response = pool
            .query(request, QueryDeadline::new(Duration::from_secs(1)))
            .await
            .expect("query should return the mock response");

        assert_eq!(response.id(), 21);
        assert_eq!(conn.query_calls(), 1);
        assert_eq!(pool.connections.len(), 1);
    }

    #[tokio::test]
    async fn test_query_closes_borrowed_connection_when_deadline_expires() {
        let pool = make_pool(0, 1, 10, MockBuilder::new(vec![]));
        let conn =
            Arc::new(MockConnection::new(true, 0, 0).with_query_delay(Duration::from_secs(60)));
        pool.connections
            .push(conn.clone())
            .expect("queue should accept connection");
        pool.active_count.store(1, Ordering::Relaxed);
        let mut request = Message::new();
        request.set_id(22);

        let result = pool
            .query(request, QueryDeadline::new(Duration::from_millis(10)))
            .await;

        assert!(result.is_err());
        assert_eq!(conn.query_calls(), 1);
        assert_eq!(conn.close_calls(), 1);
        assert_eq!(pool.connections.len(), 0);
        assert_eq!(pool.active_count.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn test_get_rechecks_capacity_after_active_connection_closes() {
        let replacement = Arc::new(MockConnection::new(true, 0, 0));
        let pool = Arc::new(make_pool(
            0,
            1,
            10,
            MockBuilder::new(vec![Ok(replacement.clone())]),
        ));
        pool.active_count.store(1, Ordering::Relaxed);

        let waiting_pool = pool.clone();
        let expected_replacement = replacement.clone();
        let waiter = tokio::spawn(async move {
            let selected = waiting_pool
                .get(QueryDeadline::new(Duration::from_secs(1)))
                .await?;
            let matched = Arc::ptr_eq(
                selected
                    .conn
                    .as_ref()
                    .expect("borrowed connection should exist"),
                &expected_replacement,
            );
            selected.release();
            Ok::<_, DnsError>(matched)
        });
        tokio::time::sleep(Duration::from_millis(10)).await;

        pool.close_active_connection(Arc::new(MockConnection::new(true, 0, 0)));

        let matched = tokio::time::timeout(Duration::from_secs(1), waiter)
            .await
            .expect("waiting get should wake after active count drops")
            .expect("join should succeed")
            .expect("get should create a replacement connection");

        assert!(matched);
        assert_eq!(pool.active_count.load(Ordering::Relaxed), 1);
    }
}
