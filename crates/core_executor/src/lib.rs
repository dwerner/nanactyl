//! Implements some convenient types for async control flow and execution of
//! async tasks on specific threads which are asked to maintain affinity to the
//! cores they were started on.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread::JoinHandle;

use async_channel::Sender;
use async_executor::LocalExecutor;
use async_oneshot::Closed;
use enrich::CoreFuture;
use futures_lite::{future, FutureExt, StreamExt};

pub mod channel;
mod enrich;

/// ThreadPoolExecutor is a high-level struct that manages a set of
/// ThreadAffineExecutors, one per core. It enables spawning tasks on specific
/// cores or any core in a round-robin fashion.
///
/// The main purpose of this executor is to allow execution of CPU-heavy tasks
/// on a threadpool while allowing code to be composed with async/await.
/// This allows for the efficient execution of tasks
/// that require affinity to specific hardware cores, while not blocking threads
/// in other executors that may be waiting on IO etc. This can be useful in
/// cases where the application requires IO-heavy tasks but also performs
/// not-insignificant work on the CPU as well. Work should be kept to separate
/// executors to avoid blocking IO tasks on CPU bound work.
///
/// # Examples
///
/// ```
/// use core_executor::ThreadPoolExecutor;
/// let mut executor = ThreadPoolExecutor::new(8);
/// ```
///
/// Spawning a task on a specific core:
/// ```
/// use core_executor::ThreadPoolExecutor;
/// let mut executor = ThreadPoolExecutor::new(8);
/// let mut spawner = executor.spawner_for_core(0).unwrap();
/// let future = spawner.spawn(async { /* ... */ });
/// futures_lite::future::block_on(future);
/// ```
///
/// Spawning a task on any core:
/// ```
/// use core_executor::ThreadPoolExecutor;
/// let mut executor = ThreadPoolExecutor::new(8);
/// let (core_id, future) = executor.spawn_on_any(async { /* ... */ });
/// futures_lite::future::block_on(future);
/// ```
pub struct ThreadPoolExecutor {
    thread_executors: Vec<ThreadAffineExecutor>,
    next_thread: AtomicUsize,
}

/// ThreadAffineExecutor represents an executor that runs on a specific core. It
/// spawns an executor thread with the provided core_id and manages task
/// execution on that core.
///
/// Alternative name: ThreadBoundExecutor
pub struct ThreadAffineExecutor {
    _core_id: usize,
    pub spawner: ThreadAffineSpawner,
    exec_thread_jh: Option<JoinHandle<()>>,
}

/// ThreadAffineSpawner is a handle to a ThreadAffineExecutor, which allows for
/// spawning tasks on the associated core. It can be cloned and sent to other
/// threads for relaying work to the underlying ThreadAffineExecutor.
///
/// Alternative name: ThreadBoundSpawner
pub struct ThreadAffineSpawner {
    pub core_id: usize,
    tx: Sender<ExecutorTask>,
    task_killers: Vec<channel::TaskShutdownHandle>,
}

type PinnedTask = Pin<Box<dyn Future<Output = ()> + Send>>;
/// ExecutorTask represents a single task to be executed by a ThreadExecutor.
enum ExecutorTask {
    Exit,
    Task(PinnedTask),
}

// Implementations

impl ThreadAffineExecutor {
    pub fn new(core_id: usize) -> Self {
        let (tx, mut rx) = async_channel::bounded::<ExecutorTask>(100);
        let exec_thread_jh = std::thread::spawn(move || {
            core_affinity::set_for_current(core_affinity::CoreId { id: core_id });
            let local_exec = LocalExecutor::new();
            future::block_on(async move {
                loop {
                    if let Some(thread_control_flow) = rx.next().await {
                        match thread_control_flow {
                            ExecutorTask::Task(task) => {
                                let task = local_exec.spawn(task);
                                local_exec.run(task).await;
                            }
                            ExecutorTask::Exit => break,
                        }
                    }
                }
            });
        });
        let exec_thread_jh = Some(exec_thread_jh);
        Self {
            _core_id: core_id,
            spawner: ThreadAffineSpawner {
                core_id,
                tx,
                task_killers: Vec::new(),
            },
            exec_thread_jh,
        }
    }
}

impl Drop for ThreadAffineExecutor {
    fn drop(&mut self) {
        if let Some(thread_handle) = self.exec_thread_jh.take() {
            self.spawner.tx.try_send(ExecutorTask::Exit).unwrap();
            thread_handle.join().unwrap();
        }
    }
}

impl Clone for ThreadAffineSpawner {
    fn clone(&self) -> Self {
        Self {
            core_id: self.core_id,
            tx: self.tx.clone(),
            // We DON'T carry forward killers on clone, so only one spawner is responsible for
            // cleanup. This could be refactored into using Arc...
            task_killers: Vec::new(),
        }
    }
}

impl ThreadPoolExecutor {
    /// Create a new ThreadPoolExecutor with the specified number of cores.
    pub fn new(cores: usize) -> Self {
        let thread_executors = (0..cores)
            .map(ThreadAffineExecutor::new)
            .collect::<Vec<_>>();
        ThreadPoolExecutor {
            thread_executors,
            next_thread: AtomicUsize::new(0),
        }
    }

    pub fn spawn_on_core<F>(
        &mut self,
        core_id: usize,
        task: F,
    ) -> CoreFuture<impl Future<Output = Result<F::Output, Closed>>>
    where
        F: Future + Send + 'static,
        F::Output: std::fmt::Debug + Send + Sync + 'static,
    {
        let spawner = &mut self.thread_executors[core_id].spawner;
        let future = spawner.spawn(task);
        CoreFuture::new(core_id, future)
    }

    pub fn spawn_on_any<F>(
        &mut self,
        task: F,
    ) -> CoreFuture<impl Future<Output = Result<F::Output, Closed>>>
    where
        F: Future + Send + 'static,
        F::Output: std::fmt::Debug + Send + Sync + 'static,
    {
        let thread_index =
            self.next_thread.fetch_add(1, Ordering::Relaxed) % self.thread_executors.len();
        let spawner = &mut self.thread_executors[thread_index].spawner;
        let future = spawner.spawn(task);
        CoreFuture::new(spawner.core_id, future)
    }
}

/// Spawner for CoreExecutor - can be send to other threads for relaying work to
/// this executor.
impl ThreadAffineSpawner {
    /// Spawn a task with a shutdown guard. When dropped, the TaskShutdown
    /// struct will ensure that this task is joined on before allowing the
    /// tracking side thread to continue.
    ///
    /// The contract here is: if a persistent task is needed, be sure to check
    /// `shutdown.should_exit()`, allowing the tracking state to trigger a
    /// shutdown if required. Long-running tasks are joined on, and
    /// therefore will block at `unload` of a plugin.
    ///
    /// Note on safety:
    ///     If a plugin starts a long-lived task (i.e. one that allows the task
    /// to live longer than the enclosing scope), it can do so safely ONLY
    /// IF it is stopped before the plugin is unloaded. Think of it as: once
    /// the compiled code for a given task (i.e. the compiled plugin) has
    /// been unloaded, any further execution of the task will result in a
    /// memory violation/segfault.
    ///
    /// This is an example of the unsafe-ness of loading plugins in general, as
    /// the borrow-checker cannot know the lifetimes of things at compile
    /// time when we are loading types and dependent code at runtime.
    // TODO: move this into a doctest
    //```
    //    state.spawn_with_shutdown(|shutdown| async move {
    //        let mut ctr = 0;
    //        loop {
    //            ctr += 1;
    //            println!(
    //                "{} long-lived task fired by ({:?})",
    //                ctr,
    //                std::thread::current().id()
    //            );
    //            smol::Timer::after(Duration::from_millis(250)).await;
    //            if shutdown.should_exit() {
    //                break;
    //            }
    //        }
    //    });
    //```
    pub fn spawn_with_shutdown<T, F>(&mut self, task_fn: T)
    where
        F: Future<Output = ()> + Send + 'static,
        T: FnOnce(channel::TaskWithShutdown) -> F,
    {
        let (killer, shutdown) = channel::TaskWithShutdown::new();
        self.fire(task_fn(shutdown));
        self.task_killers.push(killer);
    }

    pub fn spawn<F>(&mut self, task: F) -> impl Future<Output = Result<F::Output, Closed>>
    where
        F: Future + Send + 'static,
        F::Output: std::fmt::Debug + Send + Sync + 'static,
    {
        let (mut spawned_tx, spawned_rx) = async_oneshot::oneshot();
        self.tx
            .try_send(ExecutorTask::Task(
                async move {
                    if let Err(err) = spawned_tx.send(task.await) {
                        panic!("unable to send task result: {err:?}");
                    }
                }
                .boxed(),
            ))
            .expect("unable to execute task");
        spawned_rx
    }

    pub fn fire(&mut self, task: impl Future<Output = ()> + Send + 'static) {
        self.tx
            .try_send(ExecutorTask::Task(task.boxed()))
            .expect("unable to execute task");
    }

    fn block_and_kill_tasks(&mut self) {
        for kill_send in self.task_killers.drain(..) {
            kill_send.shutdown_blocking().expect("unable to kill task");
        }
    }
}

impl Drop for ThreadAffineSpawner {
    fn drop(&mut self) {
        self.block_and_kill_tasks();
    }
}

#[cfg(test)]
mod tests {

    use std::pin::Pin;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    use async_executor::LocalExecutor;
    use futures_lite::{future, FutureExt};
    use futures_util::future::join_all;
    use smol::Timer;

    use super::*;
    use crate::channel::Bichannel;

    #[test]
    fn test_bichannel() {
        let mut exec = ThreadPoolExecutor::new(2);
        let (left, right): (Bichannel<(), i16>, Bichannel<i16, ()>) = Bichannel::bounded(1);

        let left_task = exec.spawn_on_core(0, async move {
            left.send(()).await.unwrap();
            left.recv().await.unwrap()
        });
        let right_task = exec.spawn_on_core(1, async move {
            right.recv().await.unwrap();
            right.send(42).await.unwrap()
        });
        let (left_result, right_result) =
            future::block_on(futures_util::future::join(left_task, right_task));
        assert_eq!(left_result, Ok(42));
        assert_eq!(right_result, Ok(()));
    }

    #[test]
    fn test_core_executor() {
        let mut exec = ThreadPoolExecutor::new(2);
        let answer_rx = exec.spawn_on_any(Box::pin(async { 42 }));
        let answer_value = future::block_on(answer_rx);
        assert_eq!(answer_value, Ok(42));
    }

    #[test]
    fn playground_impl_channel_of_tasks() {
        let (tx, rx) =
            std::sync::mpsc::channel::<Pin<Box<dyn Future<Output = ThreadShould> + Send + '_>>>();
        println!("current thread {:?}", std::thread::current().id());
        enum ThreadShould {
            Exit,
            Continue,
        }
        let jh = std::thread::spawn(move || {
            let local_exec = LocalExecutor::new();
            loop {
                let task = rx.recv().unwrap();
                println!(
                    "running task on current thread {:?}",
                    std::thread::current().id()
                );
                match future::block_on(local_exec.run(local_exec.spawn(task))) {
                    ThreadShould::Continue => (),
                    ThreadShould::Exit => break,
                }
            }
            println!("ending executor thread");
        });
        for x in 0..10 {
            let (mut task_tx, task_rx) = async_oneshot::oneshot();
            println!("sending task from thread {:?}", std::thread::current().id());
            tx.send(
                async move {
                    for thing in 1..5 {
                        println!("thing {thing} x {x}");
                    }
                    task_tx.send(42).unwrap();
                    ThreadShould::Continue
                }
                .boxed(),
            )
            .unwrap();
            let answer = future::block_on(task_rx);
            assert_eq!(answer, Ok(42));
        }

        let sender = tx.clone();
        let other_jh = std::thread::spawn(move || {
            for _ in 1..10 {
                println!("sending from thread {:?}", std::thread::current().id());
                sender
                    .send(async { ThreadShould::Continue }.boxed())
                    .unwrap();
            }
        });

        other_jh.join().unwrap();
        tx.send(async { ThreadShould::Exit }.boxed()).unwrap();

        jh.join().unwrap();
        println!("ending thread {:?}", std::thread::current().id());
    }

    #[test]
    fn test_core_executor_spawn_specific_core() {
        let cores = 4;
        let mut executor = ThreadPoolExecutor::new(cores);
        let executed_tasks = Arc::new(AtomicUsize::new(0));
        let mut task_handles = Vec::new();

        for core_id in 0..cores {
            for _ in 0..10 {
                let executed_tasks = Arc::clone(&executed_tasks);
                let handle = executor.spawn_on_core(core_id, async move {
                    Timer::after(Duration::from_millis(100)).await;
                    executed_tasks.fetch_add(1, Ordering::Relaxed);
                });

                task_handles.push(handle);
            }
        }

        // Join on all the tasks
        future::block_on(join_all(task_handles));

        // Ensure that all tasks have been executed
        assert_eq!(executed_tasks.load(Ordering::Relaxed), cores * 10);
    }

    #[test]
    fn test_core_executor_spawn_on_any() {
        let cores = 4;
        let mut executor = ThreadPoolExecutor::new(cores);
        let executed_tasks = Arc::new(AtomicUsize::new(0));
        let mut task_handles = Vec::new();

        for _ in 0..(cores * 10) {
            let executed_tasks = Arc::clone(&executed_tasks);
            let core_future = executor.spawn_on_any(async move {
                Timer::after(Duration::from_millis(100)).await;
                executed_tasks.fetch_add(1, Ordering::Relaxed);
            });

            task_handles.push(core_future);
        }

        // Join on all the tasks
        future::block_on(join_all(task_handles));

        // Ensure that all tasks have been executed
        assert_eq!(executed_tasks.load(Ordering::Relaxed), cores * 10);
    }
}
