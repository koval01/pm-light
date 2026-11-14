use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

/// A read that takes a moment, counts how often it went, and answers with that count.
async fn slow(went: &AtomicUsize) -> Result<usize> {
    let count = went.fetch_add(1, Ordering::SeqCst) + 1;
    tokio::time::sleep(Duration::from_millis(60)).await;
    Ok(count)
}

#[tokio::test]
async fn the_same_read_asked_at_once_goes_once() {
    let (together, went) = (Together::default(), AtomicUsize::new(0));
    let (first, second, third) = tokio::join!(
        together.ask("/a", || slow(&went)),
        together.ask("/a", || slow(&went)),
        together.ask("/a", || slow(&went)),
    );
    assert_eq!(went.load(Ordering::SeqCst), 1, "a read already out went again");
    assert_eq!((first.ok(), second.ok(), third.ok()), (Some(1), Some(1), Some(1)));
    assert_eq!(together.out_now(), 0, "an answered read stayed in the list");
}

#[tokio::test]
async fn nothing_is_kept_after_the_answer() {
    let (together, went) = (Together::default(), AtomicUsize::new(0));
    assert_eq!(together.ask("/a", || slow(&went)).await.ok(), Some(1));
    assert_eq!(together.ask("/a", || slow(&went)).await.ok(), Some(2), "a later read got an old answer");
    assert_eq!(together.out_now(), 0);
}

#[tokio::test]
async fn different_reads_go_each() {
    let (together, went) = (Together::default(), AtomicUsize::new(0));
    let (first, second) = tokio::join!(together.ask("/a", || slow(&went)), together.ask("/b", || slow(&went)));
    assert_eq!(went.load(Ordering::SeqCst), 2);
    assert!(first.is_ok() && second.is_ok() && first.ok() != second.ok());
}

#[tokio::test]
async fn a_read_after_a_change_does_not_join_one_from_before() {
    let (together, went) = (Together::default(), AtomicUsize::new(0));
    let after = async {
        together.forget();
        together.ask("/a", || slow(&went)).await
    };
    let (before, after) = tokio::join!(together.ask("/a", || slow(&went)), after);
    assert_eq!(went.load(Ordering::SeqCst), 2, "the read after a change took the answer from before it");
    assert_eq!((before.ok(), after.ok()), (Some(1), Some(2)));
    assert_eq!(together.out_now(), 0);
}

#[tokio::test]
async fn a_read_dropped_on_its_way_is_not_left_behind() {
    let (together, went) = (Together::default(), AtomicUsize::new(0));
    let dropped = tokio::time::timeout(Duration::from_millis(10), together.ask("/a", || slow(&went))).await;
    assert!(dropped.is_err());
    assert_eq!(together.out_now(), 0, "a dropped read stayed in the list");
    assert_eq!(together.ask("/a", || slow(&went)).await.ok(), Some(2), "the next read hung or took a dead one");
}

#[tokio::test]
async fn when_the_one_asking_is_dropped_another_waiting_takes_over() {
    let (together, went) = (Together::default(), AtomicUsize::new(0));
    let (dropped, waiting) = tokio::join!(
        tokio::time::timeout(Duration::from_millis(10), together.ask("/a", || slow(&went))),
        together.ask("/a", || slow(&went)),
    );
    assert!(dropped.is_err());
    assert_eq!(waiting.ok(), Some(2), "the one left waiting never got an answer");
    assert_eq!(together.out_now(), 0);
}

#[tokio::test]
async fn a_failure_is_shared_like_an_answer() {
    let (together, went) = (Together::<usize>::default(), AtomicUsize::new(0));
    let failing = || async {
        went.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(60)).await;
        Err(Error::Api { status: 500, code: 0, message: String::new() })
    };
    let (first, second) = tokio::join!(together.ask("/a", failing), together.ask("/a", failing));
    assert_eq!(went.load(Ordering::SeqCst), 1, "a failing read went again for each who asked");
    assert!(matches!(first, Err(Error::Api { status: 500, .. })), "{first:?}");
    assert!(matches!(second, Err(Error::Api { status: 500, .. })), "{second:?}");
    assert_eq!(together.out_now(), 0);
}

static COPIES: AtomicUsize = AtomicUsize::new(0);

/// An answer that counts its copies, as a large one would cost them.
#[derive(Debug, PartialEq, Eq)]
struct Large(usize);

impl Clone for Large {
    fn clone(&self) -> Self {
        COPIES.fetch_add(1, Ordering::SeqCst);
        Self(self.0)
    }
}

#[tokio::test]
async fn the_last_one_waiting_takes_the_answer_whole() {
    let together = Together::default();
    let large = || async {
        tokio::time::sleep(Duration::from_millis(60)).await;
        Ok(Large(7))
    };
    assert_eq!(together.ask("/a", large).await.ok(), Some(Large(7)));
    assert_eq!(COPIES.load(Ordering::SeqCst), 0, "one asking alone paid for a copy");
    let (first, second) = tokio::join!(together.ask("/b", large), together.ask("/b", large));
    assert_eq!((first.ok(), second.ok()), (Some(Large(7)), Some(Large(7))));
    assert_eq!(COPIES.load(Ordering::SeqCst), 1, "two waiting cost more than one copy");
}
