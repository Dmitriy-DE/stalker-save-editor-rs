//! Tests for background task manager, worker cancellation, progress reporting, and non-blocking event receipt.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use sse_app::tasks::{TaskEvent, TaskManager};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

#[test]
fn task_execution_and_completion() {
    let manager = TaskManager::new();
    let handle = manager.spawn("simple_calc", |_ctx| Ok(42_u64));

    let mut completed = false;
    for _ in 0..50 {
        thread::sleep(Duration::from_millis(10));
        let events = manager.poll_events();
        for event in events {
            if let TaskEvent::Completed(id, payload) = event {
                if id == handle.id() {
                    let val = payload.downcast_ref::<u64>().expect("downcast u64");
                    assert_eq!(*val, 42);
                    completed = true;
                }
            }
        }
        if completed {
            break;
        }
    }

    assert!(completed, "task must complete and report result");
}

#[test]
fn task_progress_reporting() {
    let manager = TaskManager::new();
    let handle = manager.spawn("with_progress", |ctx| {
        ctx.report_progress(Some(0.25), Some("Quarter done".to_owned()));
        thread::sleep(Duration::from_millis(15));
        ctx.report_progress(Some(0.75), Some("Three quarters done".to_owned()));
        thread::sleep(Duration::from_millis(15));
        ctx.report_progress(Some(1.0), Some("Finished".to_owned()));
        Ok("done".to_owned())
    });

    let mut progress_reports = Vec::new();
    let mut completed = false;

    for _ in 0..50 {
        thread::sleep(Duration::from_millis(10));
        for event in manager.poll_events() {
            match event {
                TaskEvent::Progress(prog) if prog.task_id == handle.id() => {
                    progress_reports.push(prog);
                }
                TaskEvent::Completed(id, _) if id == handle.id() => {
                    completed = true;
                }
                _ => {}
            }
        }
        if completed {
            break;
        }
    }

    assert!(completed);
    assert!(!progress_reports.is_empty());
    assert!(progress_reports.iter().any(|p| p.fraction == Some(0.25)));
    assert!(progress_reports.iter().any(|p| p.fraction == Some(1.0)));
}

#[test]
fn task_cancellation_honored() {
    let manager = TaskManager::new();
    let iterations = Arc::new(AtomicU32::new(0));
    let iters_clone = iterations.clone();

    let handle = manager.spawn("cancellable_work", move |ctx| {
        while !ctx.is_cancelled() {
            iters_clone.fetch_add(1, Ordering::Relaxed);
            thread::sleep(Duration::from_millis(5));
            if iters_clone.load(Ordering::Relaxed) > 1000 {
                break;
            }
        }
        Ok(())
    });

    // Let it run a few iterations then cancel
    thread::sleep(Duration::from_millis(20));
    handle.cancel();
    assert!(handle.is_cancelled());

    let mut got_cancelled_event = false;
    for _ in 0..50 {
        thread::sleep(Duration::from_millis(10));
        for event in manager.poll_events() {
            if let TaskEvent::Cancelled(id) = event {
                if id == handle.id() {
                    got_cancelled_event = true;
                }
            }
        }
        if got_cancelled_event {
            break;
        }
    }

    assert!(got_cancelled_event, "cancellation event must be emitted");
}

#[test]
fn task_failure_reporting() {
    let manager = TaskManager::new();
    let handle = manager.spawn("failing_task", |_ctx| -> Result<(), String> {
        Err("file not found".to_owned())
    });

    let mut got_failure = false;
    for _ in 0..50 {
        thread::sleep(Duration::from_millis(10));
        for event in manager.poll_events() {
            if let TaskEvent::Failed(id, err) = event {
                if id == handle.id() {
                    assert_eq!(err, "file not found");
                    got_failure = true;
                }
            }
        }
        if got_failure {
            break;
        }
    }

    assert!(got_failure, "failure event must be received");
}

#[test]
fn ui_thread_never_blocks_on_poll() {
    let manager = TaskManager::new();
    // Polling with no tasks returns immediately
    let events = manager.poll_events();
    assert!(events.is_empty());
}
