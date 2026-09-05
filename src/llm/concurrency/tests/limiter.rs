//! Capacity, shared ownership, and request concurrency.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::llm::concurrency::Limiter;

#[tokio::test]
async fn acquire_reduces_and_drop_restores_availability() {
    let limiter = Limiter::new(5);
    assert_eq!(limiter.available(), 5);
    {
        let _first = limiter.acquire().await;
        assert_eq!(limiter.available(), 4);
        let _second = limiter.acquire().await;
        assert_eq!(limiter.available(), 3);
    }
    assert_eq!(limiter.available(), 5);
}

#[tokio::test]
async fn clones_share_capacity_and_wait_for_a_released_permit() {
    let limiter = Limiter::new(1);
    let peer = limiter.clone();
    let held = limiter.acquire().await;
    let mut waiting = std::pin::pin!(peer.acquire());
    assert!(futures::poll!(waiting.as_mut()).is_pending());
    drop(held);
    let _resumed = waiting.await;
    assert_eq!(limiter.available(), 0);
}

#[tokio::test]
async fn peak_concurrency_never_exceeds_k() {
    const N: usize = 50;
    const K: usize = 4;
    let limiter = Limiter::new(K);
    let in_flight = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let mut handles = Vec::with_capacity(N);
    for _ in 0..N {
        let limiter = limiter.clone();
        let in_flight = in_flight.clone();
        let peak = peak.clone();
        handles.push(tokio::spawn(async move {
            let _guard = limiter.acquire().await;
            let now = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            peak.fetch_max(now, Ordering::SeqCst);
            // Give queued tasks opportunities to contend while this slot is held.
            for _ in 0..16 {
                tokio::task::yield_now().await;
            }
            in_flight.fetch_sub(1, Ordering::SeqCst);
        }));
    }
    for handle in handles {
        handle.await.expect("task");
    }
    let observed = peak.load(Ordering::SeqCst);
    assert!(observed <= K, "peak {observed} exceeds capacity {K}");
}
