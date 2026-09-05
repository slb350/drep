//! Per-provider concurrency cap; HTTP retry/backoff belongs to the SDK.

use std::sync::Arc;

use tokio::sync::{Semaphore, SemaphorePermit};

/// A bounded concurrency limiter.
///
/// Clones share the same permit pool.
#[derive(Clone, Debug)]
pub struct Limiter {
    semaphore: Arc<Semaphore>,
}

/// Holds a slot until dropped.
#[must_use = "dropping the guard immediately releases the slot, so a bare \
              `limiter.acquire().await;` provides no backpressure at all"]
pub struct LimiterGuard<'a> {
    _permit: Option<SemaphorePermit<'a>>,
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
    /// dropped. Acquisitions beyond the configured capacity wait for a slot.
    pub async fn acquire(&self) -> LimiterGuard<'_> {
        // This private semaphore is never closed, so acquisition cannot fail.
        LimiterGuard {
            _permit: self.semaphore.acquire().await.ok(),
        }
    }

    /// The number of slots currently available for acquisition.
    ///
    /// Used by tests to observe permit ownership during real requests.
    pub fn available(&self) -> usize {
        self.semaphore.available_permits()
    }
}

#[cfg(test)]
mod tests;
