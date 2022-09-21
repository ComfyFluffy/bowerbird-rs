use futures::{task::AtomicWaker, Future};
use std::sync::atomic::Ordering::SeqCst;
use std::{
    pin::Pin,
    sync::{atomic::AtomicUsize, Arc},
    task::{Context, Poll},
};

#[derive(Debug)]
struct WaitGroupInner {
    num: AtomicUsize,
    waker: AtomicWaker,
}

/// A Golang-like waitgroup to wait for all tasks to complete.
#[derive(Debug, Clone)]
pub struct WaitGroup(Arc<WaitGroupInner>);

impl WaitGroup {
    // Inspired by an example in https://docs.rs/futures/0.3.17/futures/task/struct.AtomicWaker.html
    pub fn new() -> Self {
        Self(Arc::new(WaitGroupInner {
            num: AtomicUsize::new(0),
            waker: AtomicWaker::new(),
        }))
    }

    pub fn add(&self, n: usize) {
        self.0.num.fetch_add(n, SeqCst);
    }

    pub fn done(&self) {
        if self.0.num.fetch_sub(1, SeqCst) <= 1 {
            self.0.waker.wake();
        }
    }
}

impl Default for WaitGroup {
    fn default() -> Self {
        Self::new()
    }
}

impl Future for WaitGroup {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.0.num.load(SeqCst) == 0 {
            return Poll::Ready(());
        }
        self.0.waker.register(cx.waker());
        if self.0.num.load(SeqCst) == 0 {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}
