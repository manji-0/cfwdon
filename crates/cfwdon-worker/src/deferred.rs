//! Request side effects that run after the response is returned.
//!
//! Handlers push owned futures with [`defer`]; the fetch entry point hands
//! [`run_deferred_tasks_inline`] to `Context::wait_until` once the response is
//! built. Tasks must own
//! their captures (cloned `Env`, owned strings) because they outlive the
//! handler.
//!
//! Requests can interleave on one isolate, so the queue is never cleared at
//! request start: a drain may pick up work another in-flight request pushed,
//! which still runs under some request's `wait_until`. Scheduled and queue
//! handlers drain the queue inline before they return.

use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;

pub(crate) type DeferredTask = Pin<Box<dyn Future<Output = ()>>>;

thread_local! {
    static DEFERRED_TASKS: RefCell<Vec<DeferredTask>> = const { RefCell::new(Vec::new()) };
}

/// Queue `task` to run after the response is returned.
pub(crate) fn defer(task: impl Future<Output = ()> + 'static) {
    DEFERRED_TASKS.with(|tasks| tasks.borrow_mut().push(Box::pin(task)));
}

/// Take every queued task.
pub(crate) fn take_deferred_tasks() -> Vec<DeferredTask> {
    DEFERRED_TASKS.with(|tasks| std::mem::take(&mut *tasks.borrow_mut()))
}

/// Run queued tasks until none remain, including tasks queued while running.
pub(crate) async fn run_deferred_tasks_inline() {
    loop {
        let tasks = take_deferred_tasks();
        if tasks.is_empty() {
            return;
        }
        futures_util::future::join_all(tasks).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn take_drains_queued_tasks() {
        defer(async {});
        defer(async {});
        assert_eq!(take_deferred_tasks().len(), 2);
        assert!(take_deferred_tasks().is_empty());
    }
}
