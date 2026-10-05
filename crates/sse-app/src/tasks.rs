//! Background worker pool with cancellation, progress reporting, and UI notifications.
//!
//! Rule: The interface thread NEVER blocks or waits.
//! All background operations communicate results and progress back to the UI thread
//! via standard non-blocking message queues (`std::sync::mpsc`).

use std::any::Any;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// Unique identifier for a background task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TaskId(pub u64);

static NEXT_TASK_ID: AtomicU64 = AtomicU64::new(1);

impl TaskId {
    /// Allocates a new distinct task ID.
    #[must_use]
    pub fn next() -> Self {
        Self(NEXT_TASK_ID.fetch_add(1, Ordering::Relaxed))
    }
}

/// Token used to check whether a task has been cancelled.
#[derive(Clone, Debug, Default)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    /// Creates a new uncancelled token.
    #[must_use]
    pub fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Requests cancellation for this token and any tasks holding it.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    /// Returns `true` if cancellation has been requested.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

/// Progress notification emitted by a running background task.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskProgress {
    /// Task identifier.
    pub task_id: TaskId,
    /// Progress fraction between 0.0 and 1.0, if quantifiable.
    pub fraction: Option<f32>,
    /// Step description or status message.
    pub message: Option<String>,
}

/// Handle given to the worker closure to report progress and check cancellation.
#[derive(Clone)]
pub struct TaskContext {
    task_id: TaskId,
    cancellation: CancellationToken,
    progress_sender: Sender<TaskEvent>,
}

impl TaskContext {
    /// Returns the task identifier.
    #[must_use]
    pub fn task_id(&self) -> TaskId {
        self.task_id
    }

    /// Checks if cancellation was requested.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancellation.is_cancelled()
    }

    /// Reports incremental progress (fraction 0.0..1.0 and optional message).
    pub fn report_progress(&self, fraction: Option<f32>, message: Option<String>) {
        let _ = self.progress_sender.send(TaskEvent::Progress(TaskProgress {
            task_id: self.task_id,
            fraction,
            message,
        }));
    }
}

/// Events sent from background worker tasks to the interface thread.
pub enum TaskEvent {
    /// Task started executing.
    Started(TaskId),
    /// Task reported progress.
    Progress(TaskProgress),
    /// Task completed successfully with a payload.
    Completed(TaskId, Box<dyn Any + Send>),
    /// Task failed with an error message.
    Failed(TaskId, String),
    /// Task was explicitly cancelled.
    Cancelled(TaskId),
}

/// Handle retained by the caller to control a submitted task.
pub struct TaskHandle {
    task_id: TaskId,
    cancellation: CancellationToken,
}

impl TaskHandle {
    /// Returns the ID of the task.
    #[must_use]
    pub fn id(&self) -> TaskId {
        self.task_id
    }

    /// Requests cancellation of this task.
    pub fn cancel(&self) {
        self.cancellation.cancel();
    }

    /// Checks if cancellation was requested.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancellation.is_cancelled()
    }
}

type NamedTaskRegistry = (Mutex<HashMap<&'static str, usize>>, Condvar);

fn named_task_registry() -> &'static NamedTaskRegistry {
    static REGISTRY: OnceLock<NamedTaskRegistry> = OnceLock::new();
    REGISTRY.get_or_init(|| (Mutex::new(HashMap::new()), Condvar::new()))
}

struct NamedTaskGuard {
    name: &'static str,
}

impl NamedTaskGuard {
    fn enter(name: &'static str) -> Self {
        let (lock, _) = named_task_registry();
        let mut active = lock.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let count = active.entry(name).or_insert(0);
        *count = count.saturating_add(1);
        Self { name }
    }
}

impl Drop for NamedTaskGuard {
    fn drop(&mut self) {
        let (lock, changed) = named_task_registry();
        let mut active = lock.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(count) = active.get_mut(self.name) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                active.remove(self.name);
            }
        }
        changed.notify_all();
    }
}

/// Spawns a detached named worker tracked for shutdown deferral.
pub fn spawn_named_detached<F>(name: &'static str, work: F)
where
    F: FnOnce() + Send + 'static,
{
    let guard = NamedTaskGuard::enter(name);
    let _ = thread::Builder::new().name(format!("sse-{name}")).spawn(move || {
        let _guard = guard;
        work();
    });
}

/// Returns whether any worker with this task name is currently executing.
#[must_use]
pub fn named_task_active(name: &str) -> bool {
    let (lock, _) = named_task_registry();
    let active = lock.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    active.get(name).copied().unwrap_or_default() != 0
}

/// Waits only during application shutdown for named workers to finish.
///
/// Returns `true` if all matching workers completed before the deadline.
#[must_use]
pub fn wait_for_named_tasks(names: &[&str], timeout: Duration) -> bool {
    let (lock, changed) = named_task_registry();
    let now = Instant::now();
    let deadline = now.checked_add(timeout).unwrap_or(now);
    let mut active = lock.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    loop {
        if names
            .iter()
            .all(|name| active.get(name).copied().unwrap_or_default() == 0)
        {
            return true;
        }
        let now = Instant::now();
        if now >= deadline {
            return false;
        }
        let remaining = deadline.saturating_duration_since(now);
        let waited = changed.wait_timeout(active, remaining);
        match waited {
            Ok((next, result)) => {
                active = next;
                if result.timed_out() {
                    return names
                        .iter()
                        .all(|name| active.get(name).copied().unwrap_or_default() == 0);
                }
            }
            Err(poisoned) => {
                let (next, _) = poisoned.into_inner();
                active = next;
            }
        }
    }
}

type TaskEntry = (CancellationToken, JoinHandle<()>);

/// Manages background task execution and non-blocking event dispatch.
///
/// Designed to satisfy the strict rule: the UI thread NEVER blocks or waits.
/// Instead, the UI thread periodically drains `poll_events()` via non-blocking `try_recv`.
pub struct TaskManager {
    event_sender: Sender<TaskEvent>,
    event_receiver: Receiver<TaskEvent>,
    tasks: Mutex<HashMap<TaskId, TaskEntry>>,
}

impl Default for TaskManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskManager {
    /// Creates a new background task manager.
    #[must_use]
    pub fn new() -> Self {
        let (event_sender, event_receiver) = mpsc::channel();
        Self {
            event_sender,
            event_receiver,
            tasks: Mutex::new(HashMap::new()),
        }
    }

    /// Submits a background task returning a typed result `R: Any + Send + 'static`.
    ///
    /// The worker closure receives a `TaskContext` which it can use to check for cancellation
    /// and send progress updates.
    ///
    /// Returns a `TaskHandle` that can be used to cancel the task.
    ///
    /// Returns the operating-system thread creation error if the task could not start.
    pub fn spawn<F, R>(&self, name: &'static str, work: F) -> std::io::Result<TaskHandle>
    where
        F: FnOnce(TaskContext) -> Result<R, String> + Send + 'static,
        R: Any + Send + 'static,
    {
        let task_id = TaskId::next();
        let cancellation = CancellationToken::new();
        let task_context = TaskContext {
            task_id,
            cancellation: cancellation.clone(),
            progress_sender: self.event_sender.clone(),
        };

        let sender = self.event_sender.clone();
        let named_guard = NamedTaskGuard::enter(name);
        let join_handle = thread::Builder::new()
            .name(format!("sse-worker-{name}"))
            .spawn(move || {
                let _named_guard = named_guard;
                let _ = sender.send(TaskEvent::Started(task_id));
                if task_context.is_cancelled() {
                    let _ = sender.send(TaskEvent::Cancelled(task_id));
                    return;
                }

                let result = work(task_context.clone());

                if task_context.is_cancelled() {
                    let _ = sender.send(TaskEvent::Cancelled(task_id));
                    return;
                }

                match result {
                    Ok(val) => {
                        let _ = sender.send(TaskEvent::Completed(task_id, Box::new(val)));
                    }
                    Err(err) => {
                        let _ = sender.send(TaskEvent::Failed(task_id, err));
                    }
                }
            })?;

        let mut tasks_lock = self.tasks.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        tasks_lock.insert(task_id, (cancellation.clone(), join_handle));

        Ok(TaskHandle { task_id, cancellation })
    }

    /// Non-blocking check for events arriving from worker threads.
    ///
    /// The UI thread calls this each tick or frame; it never blocks.
    #[must_use]
    pub fn poll_events(&self) -> Vec<TaskEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.event_receiver.try_recv() {
            if matches!(
                event,
                TaskEvent::Completed(..) | TaskEvent::Failed(..) | TaskEvent::Cancelled(..)
            ) {
                let id = match event {
                    TaskEvent::Completed(id, _) | TaskEvent::Failed(id, _) | TaskEvent::Cancelled(id) => id,
                    _ => unreachable!(),
                };
                let mut tasks_lock = self.tasks.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                if let Some((_, handle)) = tasks_lock.remove(&id) {
                    // Joining a finished thread is non-blocking.
                    if handle.is_finished() {
                        let _ = handle.join();
                    }
                }
            }
            events.push(event);
        }
        events
    }

    /// Cancels a specific task by its ID.
    pub fn cancel(&self, id: TaskId) {
        let tasks_lock = self.tasks.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((token, _)) = tasks_lock.get(&id) {
            token.cancel();
        }
    }

    /// Cancels all active tasks.
    pub fn cancel_all(&self) {
        let tasks_lock = self.tasks.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        for (token, _) in tasks_lock.values() {
            token.cancel();
        }
    }

    /// Returns the number of currently tracked tasks.
    #[must_use]
    pub fn active_task_count(&self) -> usize {
        let tasks_lock = self.tasks.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        tasks_lock.len()
    }
}

impl Drop for TaskManager {
    fn drop(&mut self) {
        self.cancel_all();
    }
}
