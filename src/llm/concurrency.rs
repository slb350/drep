//! Concurrency cap for LLM requests.
//!
//! One thing, on purpose: a bounded number of in-flight requests, nothing
//! else. A local commit gate reviewing a handful of changed files does not need
//! server-oriented rate-limit machinery:
//!
//! - **Per-repo semaphores** are a map with one entry. One invocation, one
//!   repo, one bucket.
//! - **A requests-per-minute window and a tokens-per-minute budget** are a
//!   second, worse implementation of what `open-agent-sdk` 0.7.0 already
//!   does: it classifies 429 as retryable and backs off, so a client-side
//!   throttle duplicates the backoff without owning it.
//! - **Two-step token accounting** exists to reconcile an estimate against
//!   actuals across a queue. With no token budget, nothing to reconcile, so
//!   the "never hold the lock across an await" hazard disappears with it.
//! - **A circuit breaker** protects a shared service from a stampede. One
//!   developer committing is not a stampede.
//!
//! If a real workload later shows `max_concurrent` is not enough, add the
//! narrowest mechanism that fixes it.
//!
//! [`tokio::sync::Semaphore::acquire`] returns `Result` because the
//! semaphore can be closed. This limiter never closes it, so the error
//! case is unreachable in practice; it is mapped to an empty guard rather
//! than propagated as an error the caller cannot act on.

use std::sync::Arc;

use tokio::sync::{Semaphore, SemaphorePermit};

/// A bounded concurrency limiter.
///
/// Cheap to clone (the semaphore is wrapped in an `Arc`); typically built
/// once per process and shared across the analyzer.
#[derive(Clone, Debug)]
pub struct Limiter {
    semaphore: Arc<Semaphore>,
}

/// RAII guard holding one slot in the limiter.
///
/// Drop the guard to release the slot. In the (unreachable) error case
/// where the semaphore was closed, the guard holds no permit and dropping
/// it is a no-op - the limiter cannot make a phantom permit do useful
/// work, but the type stays total so callers do not have to handle a
/// second error variant.
#[must_use = "dropping the guard immediately releases the slot, so a bare \
              `limiter.acquire().await;` provides no backpressure at all"]
pub struct LimiterGuard<'a> {
    /// The held permit, if any. `Option` so the unreachable error case
    /// can leave the guard empty; `Drop` below explicitly drops the
    /// permit so the slot returns to the pool.
    permit: Option<SemaphorePermit<'a>>,
}

impl Drop for LimiterGuard<'_> {
    fn drop(&mut self) {
        // Explicitly drop the held permit so the slot is released now
        // rather than whenever the field happens to be reclaimed. The
        // `take` also makes the field's role unambiguous to the
        // dead-code lint: without it, holding a value that is "only
        // dropped" can warn.
        drop(self.permit.take());
    }
}

impl Limiter {
    /// Build a limiter that allows at most `max_concurrent` in-flight
    /// requests. `max_concurrent = 0` is allowed by the type system but
    /// makes every `acquire` await forever; callers are expected to set a
    /// positive value from `LlmConfig::max_concurrent` (default 3).
    pub fn new(max_concurrent: usize) -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(max_concurrent)),
        }
    }

    /// Acquire a slot. The slot is held until the returned guard is
    /// dropped.
    ///
    /// Backpressure is the entire job: callers await, and once `K` requests
    /// are in flight, the next one queues here until one of the holders
    /// drops its guard.
    pub async fn acquire(&self) -> LimiterGuard<'_> {
        // `Semaphore::acquire` only returns `Err(AcquireError)` when the
        // semaphore has been closed. We never close it, so the error is
        // unreachable; mapping it to an empty guard keeps the API total
        // without resorting to `unwrap` outside tests.
        let permit = self.semaphore.acquire().await.ok();
        LimiterGuard { permit }
    }

    /// The number of slots currently available for acquisition.
    ///
    /// Reflects the configured maximum before any acquisition, decreases by
    /// one per held guard, and returns to the maximum once every guard has
    /// been dropped. Test-only - exposed via the API so tests can pin the
    /// behaviour without reaching into the semaphore.
    pub fn available(&self) -> usize {
        self.semaphore.available_permits()
    }
}

#[cfg(test)]
mod tests;
