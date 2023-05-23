use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread::{Scope, ScopedJoinHandle};

use async_channel::Sender;
use async_executor::LocalExecutor;
use async_oneshot::Closed;
use futures_lite::{future, Future, FutureExt, StreamExt};

use crate::channel;
use crate::enrich::CoreFuture;

type ScopedPinnedTask<'task> = Pin<Box<dyn Future<Output = ()> + Send + 'task>>;
/// ExecutorTask represents a single task to be executed by a ThreadExecutor.
enum ScopedExecutorTask<'task> {
    Exit,
    Task(ScopedPinnedTask<'task>),
}

/// Scoped executor - requires a std::thread::Scope
pub struct ScopedThreadAffineExecutor<'scope> {
    _core_id: usize,
    pub spawner: ScopedThreadAffineSpawner<'scope>,
    exec_thread_jh: Option<ScopedJoinHandle<'scope, ()>>,
}

pub struct ScopedThreadAffineSpawner<'task> {
    pub core_id: usize,
    tx: Sender<ScopedExecutorTask<'task>>,
    task_killers: Vec<channel::TaskShutdownHandle>,
}
// Implementations

impl<'scope> ScopedThreadAffineExecutor<'scope> {
    pub fn new(core_id: usize, scope: &'scope Scope<'scope, '_>) -> Self {
        let (tx, mut rx) = async_channel::bounded::<ScopedExecutorTask<'scope>>(100);
        let exec_thread_jh = scope.spawn(move || {
            core_affinity::set_for_current(core_affinity::CoreId { id: core_id });
            let local_exec = LocalExecutor::new();
            future::block_on(async move {
                loop {
                    if let Some(thread_control_flow) = rx.next().await {
                        match thread_control_flow {
                            ScopedExecutorTask::Task(task) => {
                                let task = local_exec.spawn(task);
                                local_exec.run(task).await;
                            }
                            ScopedExecutorTask::Exit => break,
                        }
                    }
                }
            });
        });

        let exec_thread_jh = Some(exec_thread_jh);
        Self {
            _core_id: core_id,
            spawner: ScopedThreadAffineSpawner {
                core_id,
                tx,
                task_killers: Vec::new(),
            },
            exec_thread_jh,
        }
    }
}

impl<'a> Drop for ScopedThreadAffineExecutor<'a> {
    fn drop(&mut self) {
        if let Some(thread_handle) = self.exec_thread_jh.take() {
            self.spawner.tx.try_send(ScopedExecutorTask::Exit).unwrap();
            thread_handle.join().unwrap();
        }
    }
}

impl<'task> ScopedThreadAffineSpawner<'task> {
    pub fn spawn<F>(&mut self, task: F) -> impl Future<Output = Result<F::Output, Closed>> + 'task
    where
        F: Future + Send + 'task,
        F::Output: std::fmt::Debug + Send + Sync + 'task,
    {
        let (mut spawned_tx, spawned_rx) = async_oneshot::oneshot();
        self.tx
            .try_send(ScopedExecutorTask::Task(
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
}

pub struct ScopedThreadPoolExecutor<'scope> {
    thread_executors: Vec<ScopedThreadAffineExecutor<'scope>>,
    next_thread: AtomicUsize,
}

impl<'scope> ScopedThreadPoolExecutor<'scope> {
    /// Create a new ThreadPoolExecutor with the specified number of cores.
    pub fn new(cores: usize, scope: &'scope Scope<'scope, '_>) -> Self {
        let mut thread_executors = Vec::new();
        for core_id in 0..cores {
            thread_executors.push(ScopedThreadAffineExecutor::new(core_id, scope));
        }
        ScopedThreadPoolExecutor {
            thread_executors,
            next_thread: AtomicUsize::new(0),
        }
    }

    pub fn spawn_on_core<F>(
        &mut self,
        core_id: usize,
        task: F,
    ) -> CoreFuture<impl Future<Output = Result<F::Output, Closed>> + 'scope>
    where
        F: Future + Send + 'scope,
        F::Output: std::fmt::Debug + Send + Sync + 'scope,
    {
        let future = self.thread_executors[core_id].spawner.spawn(task);
        CoreFuture::new(core_id, future)
    }

    pub fn spawn_on_any<F>(
        &mut self,
        task: F,
    ) -> CoreFuture<impl Future<Output = Result<F::Output, Closed>> + 'scope>
    where
        F: Future + Send + 'scope,
        F::Output: std::fmt::Debug + Send + Sync + 'scope,
    {
        let thread_index =
            self.next_thread.fetch_add(1, Ordering::Relaxed) % self.thread_executors.len();
        let spawner = &mut self.thread_executors[thread_index].spawner;
        let future = spawner.spawn(task);
        CoreFuture::new(spawner.core_id, future)
    }
}
