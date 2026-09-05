//! The persistent three-round semantic-review budget.

use std::collections::BTreeSet;
use std::sync::{Arc, Barrier};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::test_support::set_mtime;

use super::super::review_budget::{
    Budget, Claim, PENDING_LEASE_SECS, is_authoritative, is_completion_scope, lease_expired,
};
use super::support::check_args;

#[test]
fn pending_lease_is_exactly_seven_days_and_expires_after_the_boundary() {
    assert_eq!(PENDING_LEASE_SECS, 604_800);
    assert!(!lease_expired(1_000 + PENDING_LEASE_SECS, 1_000));
    assert!(lease_expired(1_001 + PENDING_LEASE_SECS, 1_000));
}

#[test]
fn each_complete_input_mode_is_independently_authoritative() {
    let mut args = check_args(Vec::new(), None);
    assert!(!is_completion_scope(&args));
    assert!(!is_authoritative(&args));

    args.diff = Some("origin/main".to_owned());
    assert!(is_completion_scope(&args));
    args.diff = None;

    args.pre_commit_push = true;
    assert!(is_completion_scope(&args));
    args.pre_commit_push = false;

    args.push_gate = true;
    assert!(is_completion_scope(&args));
    args.paths.push("src/lib.rs".into());
    assert!(!is_completion_scope(&args));

    args.push_gate = false;
    args.staged = true;
    assert!(!is_completion_scope(&args));
    assert!(is_authoritative(&args));
}

fn committed_round(claim: Claim) -> u32 {
    let Claim::Reserved(reservation) = claim else {
        panic!("expected an available review round");
    };
    let round = reservation.round();
    reservation.commit().expect("commit reservation");
    round
}

#[test]
fn three_committed_rounds_exhaust_the_default_budget() {
    let dir = tempfile::tempdir().expect("tempdir");
    let budget = Budget::at(dir.path(), "refs/heads/feature", 3, 1_000);

    assert_eq!(committed_round(budget.claim().expect("round 1")), 1);
    assert_eq!(committed_round(budget.claim().expect("round 2")), 2);
    assert_eq!(committed_round(budget.claim().expect("round 3")), 3);
    for round in 1..=3 {
        set_mtime(
            &budget.directory().join(format!("round-{round}.state")),
            UNIX_EPOCH + Duration::from_secs(1_000),
        );
    }
    let aged = Budget::at(
        dir.path(),
        "refs/heads/feature",
        3,
        1_001 + PENDING_LEASE_SECS,
    );
    assert!(matches!(
        aged.claim().expect("committed rounds never expire"),
        Claim::LimitReached {
            completed: 3,
            limit: 3,
        }
    ));
}

#[test]
fn dropping_an_uncommitted_reservation_refunds_the_round() {
    let dir = tempfile::tempdir().expect("tempdir");
    let budget = Budget::at(dir.path(), "refs/heads/feature", 3, 1_000);

    let Claim::Reserved(reservation) = budget.claim().expect("reservation") else {
        panic!("round must be available");
    };
    assert_eq!(reservation.round(), 1);
    drop(reservation);

    assert_eq!(committed_round(budget.claim().expect("round reused")), 1);
}

#[test]
fn a_higher_explicit_limit_allows_an_additional_round() {
    let dir = tempfile::tempdir().expect("tempdir");
    let default = Budget::at(dir.path(), "refs/heads/feature", 3, 1_000);
    for expected in 1..=3 {
        assert_eq!(
            committed_round(default.claim().expect("default round")),
            expected
        );
    }

    let extended = Budget::at(dir.path(), "refs/heads/feature", 4, 1_001);
    assert_eq!(
        committed_round(extended.claim().expect("extended round")),
        4
    );
}

#[test]
fn concurrent_claims_never_oversubscribe_the_limit() {
    let dir = tempfile::tempdir().expect("tempdir");
    let budget = Arc::new(Budget::at(dir.path(), "refs/heads/concurrent", 3, 1_000));
    let barrier = Arc::new(Barrier::new(8));
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let budget = Arc::clone(&budget);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                match budget.claim().expect("claim") {
                    Claim::Reserved(reservation) => {
                        let round = reservation.round();
                        reservation.commit().expect("commit");
                        Some(round)
                    }
                    Claim::LimitReached { .. } => None,
                }
            })
        })
        .collect();

    let rounds: BTreeSet<u32> = handles
        .into_iter()
        .filter_map(|handle| handle.join().expect("thread"))
        .collect();
    assert_eq!(rounds, BTreeSet::from([1, 2, 3]));
}

#[test]
fn concurrent_stale_reclaimers_never_replace_each_others_reservations() {
    let dir = tempfile::tempdir().expect("tempdir");
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("current time")
        .as_secs();
    let original = Budget::at(dir.path(), "refs/heads/concurrent-stale", 1, now);
    std::fs::create_dir_all(original.directory()).expect("budget directory");
    std::fs::write(original.directory().join("round-1.state"), "").expect("empty stale slot");

    let budget = Arc::new(Budget::at(
        dir.path(),
        "refs/heads/concurrent-stale",
        1,
        now + PENDING_LEASE_SECS + 1,
    ));
    let barrier = Arc::new(Barrier::new(8));
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let budget = Arc::clone(&budget);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                match budget.claim().expect("claim") {
                    Claim::Reserved(reservation) => {
                        reservation.commit().expect("commit");
                        true
                    }
                    Claim::LimitReached { .. } => false,
                }
            })
        })
        .collect();

    assert_eq!(
        handles
            .into_iter()
            .map(|handle| handle.join().expect("thread"))
            .filter(|claimed| *claimed)
            .count(),
        1
    );
}

#[test]
fn stale_pending_slot_is_reclaimed_but_a_fresh_one_is_respected() {
    let dir = tempfile::tempdir().expect("tempdir");
    let original = Budget::at(dir.path(), "refs/heads/feature", 2, 1_000);
    let Claim::Reserved(stale) = original.claim().expect("pending slot") else {
        panic!("round must be available");
    };
    assert_eq!(stale.round(), 1);
    std::mem::forget(stale); // Simulate a killed process: Drop never refunds it.

    let before_expiry = Budget::at(
        dir.path(),
        "refs/heads/feature",
        2,
        1_000 + PENDING_LEASE_SECS,
    );
    let Claim::Reserved(fresh) = before_expiry.claim().expect("second slot") else {
        panic!("the unexpired pending slot must remain occupied");
    };
    assert_eq!(fresh.round(), 2);
    drop(fresh);

    let after_expiry = Budget::at(
        dir.path(),
        "refs/heads/feature",
        2,
        1_001 + PENDING_LEASE_SECS,
    );
    let Claim::Reserved(reclaimed) = after_expiry.claim().expect("reclaimed slot") else {
        panic!("the stale pending slot must be reclaimed");
    };
    assert_eq!(reclaimed.round(), 1);
}

#[test]
fn displaced_owner_cannot_commit_or_refund_its_successors_reservation() {
    for externally_displaced in [false, true] {
        let dir = tempfile::tempdir().expect("tempdir");
        let original = Budget::at(dir.path(), "refs/heads/feature", 1, 1_000);
        let Claim::Reserved(displaced) = original.claim().expect("original reservation") else {
            panic!("round must be available");
        };

        let successor_time = if externally_displaced {
            std::fs::rename(
                original.directory().join("round-1.state"),
                original.directory().join("displaced.state"),
            )
            .expect("externally displace the original slot");
            // A timestamp establishes age; the token distinguishes owners when
            // an external replacement reuses the same slot within that second.
            1_000
        } else {
            1_001 + PENDING_LEASE_SECS
        };
        let successor_budget = Budget::at(dir.path(), "refs/heads/feature", 1, successor_time);
        let Claim::Reserved(successor) = successor_budget.claim().expect("successor reservation")
        else {
            panic!("displaced round must be available");
        };

        let err = displaced
            .commit()
            .expect_err("displaced owner must lose the slot");
        assert!(
            err.to_string().contains("is no longer owned"),
            "unexpected error: {err:#}"
        );
        successor
            .commit()
            .expect("displaced owner's Drop must preserve successor");
        assert!(matches!(
            successor_budget.claim().expect("limit verdict"),
            Claim::LimitReached {
                completed: 1,
                limit: 1,
            }
        ));
    }
}

#[test]
fn incomplete_slots_are_respected_until_their_lease_expires() {
    let dir = tempfile::tempdir().expect("tempdir");
    let now = 1_000;
    let original = Budget::at(dir.path(), "refs/heads/feature", 1, now);
    std::fs::create_dir_all(original.directory()).expect("budget directory");
    let slot = original.directory().join("round-1.state");
    let after_expiry = Budget::at(
        dir.path(),
        "refs/heads/feature",
        1,
        now + PENDING_LEASE_SECS + 1,
    );
    for raw in [b"".as_slice(), b"\xff".as_slice()] {
        std::fs::write(&slot, raw).expect("incomplete slot");
        set_mtime(&slot, UNIX_EPOCH + Duration::from_secs(now));
        assert!(
            !matches!(original.claim(), Ok(Claim::Reserved(_))),
            "a fresh incomplete slot must remain occupied"
        );
        assert_eq!(std::fs::read(&slot).expect("fresh slot survives"), raw);
        let Claim::Reserved(reclaimed) = after_expiry.claim().expect("reclaimed slot") else {
            panic!("a killed writer must not exhaust the budget forever");
        };
        assert_eq!(reclaimed.round(), 1);
    }
}

#[test]
fn reset_removes_owned_slots_without_touching_foreign_files() {
    let dir = tempfile::tempdir().expect("tempdir");
    let budget = Budget::at(dir.path(), "refs/heads/feature", 3, 1_000);
    committed_round(budget.claim().expect("round"));
    let foreign = budget.directory().join("notes.txt");
    // Matching contents must not turn an unrelated filename into an owned slot.
    std::fs::write(&foreign, "committed\n").expect("foreign file");

    assert!(budget.reset().expect("reset"));

    assert_eq!(
        committed_round(budget.claim().expect("round after reset")),
        1
    );
    assert_eq!(
        std::fs::read_to_string(foreign).expect("foreign survives"),
        "committed\n"
    );
}

#[test]
fn reset_preserves_another_process_fresh_pending_slot() {
    let dir = tempfile::tempdir().expect("tempdir");
    let budget = Budget::at(dir.path(), "refs/heads/feature", 2, 1_000);
    committed_round(budget.claim().expect("committed round"));
    let Claim::Reserved(pending) = budget.claim().expect("pending round") else {
        panic!("round must be available");
    };
    assert_eq!(pending.round(), 2);
    std::mem::forget(pending); // Another live process owns the lease.

    budget.reset().expect("reset committed state");

    let Claim::Reserved(reused) = budget.claim().expect("first round reopened") else {
        panic!("the committed slot should have been reset");
    };
    assert_eq!(reused.round(), 1);
    std::mem::forget(reused);
    assert!(matches!(
        budget.claim().expect("both live leases still count"),
        Claim::LimitReached {
            completed: 0,
            limit: 2,
        }
    ));
}

#[test]
fn separate_branch_identities_have_separate_budgets() {
    let dir = tempfile::tempdir().expect("tempdir");
    let first = Budget::at(dir.path(), "refs/heads/one", 1, 1_000);
    let second = Budget::at(dir.path(), "refs/heads/two", 1, 1_000);

    assert_eq!(committed_round(first.claim().expect("first branch")), 1);
    assert_eq!(committed_round(second.claim().expect("second branch")), 1);
    assert_ne!(first.directory(), second.directory());
}

#[tokio::test]
async fn linked_worktrees_store_their_budgets_in_separate_git_directories() {
    let parent = tempfile::tempdir().expect("tempdir");
    let main = parent.path().join("main");
    let linked = parent.path().join("linked");
    std::fs::create_dir(&main).expect("main directory");
    crate::test_support::git_init(&main);
    std::fs::write(main.join("lib.py"), "x = 1\n").expect("source");
    crate::test_support::git_add(&main, "lib.py");
    let commit = crate::test_support::git(&main)
        .args(["commit", "--quiet", "--no-verify", "-m", "base"])
        .output()
        .expect("git commit");
    assert!(commit.status.success(), "commit must succeed");
    let add_worktree = crate::test_support::git(&main)
        .args(["worktree", "add", "--quiet", "-b", "linked"])
        .arg(&linked)
        .output()
        .expect("git worktree add");
    assert!(
        add_worktree.status.success(),
        "worktree add failed: {}",
        String::from_utf8_lossy(&add_worktree.stderr)
    );

    let main_budget = Budget::for_repo(&main, 3).await.expect("main budget");
    let linked_budget = Budget::for_repo(&linked, 3).await.expect("linked budget");

    assert_ne!(main_budget.directory(), linked_budget.directory());
    assert_eq!(committed_round(main_budget.claim().expect("main round")), 1);
    assert_eq!(
        committed_round(linked_budget.claim().expect("linked round")),
        1
    );
}
