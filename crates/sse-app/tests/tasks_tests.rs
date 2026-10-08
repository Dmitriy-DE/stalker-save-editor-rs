//! Tests for background task manager, worker cancellation, progress reporting, and non-blocking event receipt.

use sse_app::tasks::{TaskEvent, TaskManager};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

#[test]
fn task_execution_and_completion() -> std::io::Result<()> {
    let manager = TaskManager::new();
    let handle = manager.spawn("simple_calc", |_ctx| Ok(42_u64))?;

    let mut completed = false;
    for _ in 0..50 {
        thread::sleep(Duration::from_millis(10));
        let events = manager.poll_events();
        for event in events {
            if let TaskEvent::Completed(id, payload) = event {
                if id == handle.id() {
                    if let Some(value) = payload.downcast_ref::<u64>() {
                        assert_eq!(*value, 42);
                        completed = true;
                    }
                }
            }
        }
        if completed {
            break;
        }
    }

    assert!(completed, "task must complete and report result");
    Ok(())
}

#[test]
fn task_progress_reporting() -> std::io::Result<()> {
    let manager = TaskManager::new();
    let handle = manager.spawn("with_progress", |ctx| {
        ctx.report_progress(Some(0.25), Some("Quarter done".to_owned()));
        thread::sleep(Duration::from_millis(15));
        ctx.report_progress(Some(0.75), Some("Three quarters done".to_owned()));
        thread::sleep(Duration::from_millis(15));
        ctx.report_progress(Some(1.0), Some("Finished".to_owned()));
        Ok("done".to_owned())
    })?;

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
    Ok(())
}

#[test]
fn task_cancellation_honored() -> std::io::Result<()> {
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
    })?;

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
    Ok(())
}

#[test]
fn task_failure_reporting() -> std::io::Result<()> {
    let log_directory = std::env::temp_dir().join(format!("sse-task-failure-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&log_directory);
    sse_app::diagnostics::configure_log_directory(Some(log_directory.clone()));

    let manager = TaskManager::new();
    let handle = manager.spawn("failing_task", |_ctx| -> Result<(), String> {
        Err("file not found: /home/alice/private.sav".to_owned())
    })?;

    let mut got_failure = false;
    let mut failure_message = String::new();
    for _ in 0..50 {
        thread::sleep(Duration::from_millis(10));
        for event in manager.poll_events() {
            if let TaskEvent::Failed(id, err) = event {
                if id == handle.id() {
                    failure_message = err;
                    got_failure = true;
                }
            }
        }
        if got_failure {
            break;
        }
    }

    sse_app::diagnostics::install_crash_reporter();
    let _panic_handle = manager.spawn("panicking_task", |_ctx| -> Result<(), String> {
        panic!("background task panicked at /home/alice/private.sav");
    })?;
    let mut panic_marker = String::new();
    // The marker is written from the panicking worker thread; slow CI runners need more than 0.5 s.
    for _ in 0..500 {
        if let Ok(marker) = std::fs::read_to_string(log_directory.join("last-crash.txt")) {
            panic_marker = marker;
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }

    let log = std::fs::read_to_string(log_directory.join("save-editor.log")).unwrap_or_default();
    sse_app::diagnostics::configure_log_directory(None);
    let _ = std::fs::remove_dir_all(log_directory);

    assert!(got_failure, "failure event must be received");
    assert_eq!(failure_message, "file not found: /home/alice/private.sav");
    assert!(log.contains("failing_task"), "task name should be logged");
    assert!(log.contains("file not found"), "failure should be logged");
    assert!(log.contains("<home>/private.sav"), "the log should redact user paths");
    assert!(!log.contains("alice"), "the log must not retain the home user name");
    assert!(
        panic_marker.contains("Unhandled panic"),
        "the panic marker should be written"
    );
    assert!(
        panic_marker.contains("<home>/private.sav"),
        "panic paths should be redacted"
    );
    Ok(())
}

#[test]
fn panicked_task_sends_a_terminal_failure_event() -> std::io::Result<()> {
    let manager = TaskManager::new();
    let handle = manager.spawn("panicking_terminal", |_ctx| -> Result<(), String> {
        panic!("intentional task panic");
    })?;

    let mut failure = None;
    for _ in 0..50 {
        for event in manager.poll_events() {
            match event {
                TaskEvent::Failed(id, message) if id == handle.id() => failure = Some(message),
                TaskEvent::Completed(id, _) if id == handle.id() => {
                    return Err(std::io::Error::other("panicked task reported success"));
                }
                _ => {}
            }
        }
        if failure.is_some() {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }

    assert_eq!(
        failure.as_deref(),
        Some("background task panicked: intentional task panic")
    );
    Ok(())
}

#[test]
fn ui_thread_never_blocks_on_poll() {
    let manager = TaskManager::new();
    // Polling with no tasks returns immediately
    let events = manager.poll_events();
    assert!(events.is_empty());
}
