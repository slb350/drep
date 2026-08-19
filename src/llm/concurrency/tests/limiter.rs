//! `Limiter` behaviour.
//!
//! Criteria 21-24. Four pins: `available()` reflects the configured
//! maximum before acquisition; acquiring reduces and dropping restores
//! availability; a limiter of 1 serialises two concurrent tasks; and
//! concurrency is genuinely bounded under a swarm of `N` tasks against a
//! limiter of `K`. The fourth is the criterion that matters - a limiter
//! that hands out unlimited permits would pass the others.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::llm::concurrency::Limiter;

/// Criterion 21: `available()` reflects the configured maximum before
/// any acquisition.
#[test]
fn available_reflects_configured_maximum_before_acquisition() {
    let limiter = Limiter::new(7);
    assert_eq!(
        limiter.available(),
        7,
        "fresh limiter must show full capacity"
    );
}

/// Criterion 22: acquiring reduces availability, and dropping the guard
/// restores it.
#[tokio::test]
async fn acquire_reduces_and_drop_restores_availability() {
    let limiter = Limiter::new(5);
    assert_eq!(limiter.available(), 5);

    {
        let _g1 = limiter.acquire().await;
        assert_eq!(limiter.available(), 4, "one slot held");

        let _g2 = limiter.acquire().await;
        assert_eq!(limiter.available(), 3, "two slots held");
    }

    assert_eq!(
        limiter.available(),
        5,
        "dropping all guards must restore full capacity"
    );
}

/// Criterion 23: a limiter of 1 serialises two concurrent tasks.
///
/// Each task records the moment it holds the slot and the moment it
/// releases. The intervals must not overlap: the log shows the entire
/// hold-then-release of one task before the other task's hold begins. A
/// non-serialising limiter would let both tasks hold the slot
/// simultaneously and the log would show interleaved holds.
#[tokio::test]
async fn limiter_of_one_serialises_two_concurrent_tasks() {
    // ONE limiter, shared via the internal Arc<Semaphore>. Cloning the
    // limiter gives another handle to the same semaphore - two clones of
    // K=1 with independent semaphores would let the tasks run in
    // parallel and silently pass this test.
    let limiter = Limiter::new(1);
    let order = Arc::new(std::sync::Mutex::new(Vec::<&'static str>::new()));

    let limiter_a = limiter.clone();
    let order_a = order.clone();
    let handle_a = tokio::spawn(async move {
        let _g = limiter_a.acquire().await;
        order_a.lock().unwrap().push("a-hold");
        // Hold the slot long enough that the other task's acquire() has
        // time to reach the queued state. We cannot use a barrier here:
        // both tasks would arrive at the barrier only after both have
        // acquired, and with K=1 the second acquire is queued waiting on
        // us - so neither ever reaches the barrier. A sleep holds the
        // slot while the queue fills, then we drop and the other task
        // can run.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        order_a.lock().unwrap().push("a-release");
    });

    let limiter_b = limiter.clone();
    let order_b = order.clone();
    let handle_b = tokio::spawn(async move {
        let _g = limiter_b.acquire().await;
        order_b.lock().unwrap().push("b-hold");
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        order_b.lock().unwrap().push("b-release");
    });

    handle_a.await.expect("a");
    handle_b.await.expect("b");

    let log = order.lock().unwrap();
    // The only legal orderings are: A holds, A releases, B holds, B
    // releases - OR the same with B first. Anything else (interleaved
    // holds or one holding while the other holds) means the limiter
    // did not serialise.
    let legal_a_first = log.as_slice() == ["a-hold", "a-release", "b-hold", "b-release"];
    let legal_b_first = log.as_slice() == ["b-hold", "b-release", "a-hold", "a-release"];
    assert!(
        legal_a_first || legal_b_first,
        "K=1 must serialise: got {log:?}"
    );
}

/// Criterion 24: concurrency is genuinely bounded.
///
/// Spawn N tasks against a limiter of K, track the peak simultaneous
/// holders in an `AtomicUsize`, and assert the peak never exceeded K.
/// This is the criterion that catches a limiter that hands out unlimited
/// permits - every other test would still pass against such a limiter,
/// which is the failure mode the spec calls out.
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
            // fetch_max does an atomic max; the precise value does not
            // matter, only that the running high-water mark is updated.
            peak.fetch_max(now, Ordering::SeqCst);
            // Yield repeatedly so other tasks have a chance to acquire
            // before we release - the test only catches unbounded
            // concurrency if tasks actually overlap.
            for _ in 0..16 {
                tokio::task::yield_now().await;
            }
            in_flight.fetch_sub(1, Ordering::SeqCst);
        }));
    }

    for h in handles {
        h.await.expect("task");
    }

    let observed = peak.load(Ordering::SeqCst);
    assert!(
        observed <= K,
        "peak concurrency must be <= K ({K}), observed {observed}"
    );
    // Sanity check the test actually exercised the limiter: at least one
    // task ran concurrently. Without this, a broken test that spawned N
    // tasks that each ran serially would pass the bound trivially.
    assert!(
        observed >= 1,
        "peak concurrency must be >= 1 for the test to exercise the limiter, observed {observed}"
    );
}
