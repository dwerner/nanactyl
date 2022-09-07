use std::{future::Future, pin::Pin, process::Output, thread::JoinHandle};

use async_channel::Sender;
use async_executor::LocalExecutor;
use async_oneshot::Closed;
use futures_lite::{future, FutureExt, StreamExt};

pub mod channel;

pub struct ThreadExecutor {
    _core_id: usize,
    // TODO: return tuple instead
    pub spawner: ThreadExecutorSpawner,
    exec_thread_jh: Option<JoinHandle<()>>,
}

impl ThreadExecutor {
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
            spawner: ThreadExecutorSpawner {
                core_id,
                tx,
                task_killers: Vec::new(),
            },
            exec_thread_jh,
        }
    }
}

impl Drop for ThreadExecutor {
    fn drop(&mut self) {
        if let Some(thread_handle) = self.exec_thread_jh.take() {
            self.spawner.tx.try_send(ExecutorTask::Exit).unwrap();
            thread_handle.join().unwrap();
        }
    }
}

pub struct CoreAffinityExecutor {
    thread_executors: Vec<ThreadExecutor>,
}

pub struct ThreadExecutorSpawner {
    pub core_id: usize,
    tx: Sender<ExecutorTask>,
    task_killers: Vec<channel::TaskShutdownHandle>,
}

impl Clone for ThreadExecutorSpawner {
    fn clone(&self) -> Self {
        Self {
            core_id: self.core_id,
            tx: self.tx.clone(),
            // We DON'T carry forward killers on clone, so only one spawner is responsible for cleanup.
            // This could be refactored into using Arc...
            task_killers: Vec::new(),
        }
    }
}

enum ExecutorTask {
    Exit,
    Task(PinnedTask),
}

type PinnedTask = Pin<Box<dyn Future<Output = ()> + Send>>;

// Must be owned by a single thread
impl CoreAffinityExecutor {
    // TODO: take a thread name, and label the thread with that for better debug experience
    pub fn new(cores: usize) -> Self {
        let thread_executors = (0..cores).map(ThreadExecutor::new).collect::<Vec<_>>();
        CoreAffinityExecutor { thread_executors }
    }

    pub fn spawners(&self) -> Vec<ThreadExecutorSpawner> {
        self.thread_executors
            .iter()
            .map(|ThreadExecutor { spawner, .. }| spawner.clone())
            .collect()
    }

    pub fn spawner_for_core(&self, id: usize) -> Option<ThreadExecutorSpawner> {
        self.thread_executors
            .iter()
            .map(|ThreadExecutor { spawner, .. }| spawner)
            .find(|ThreadExecutorSpawner { core_id, .. }| *core_id == id)
            .cloned()
    }
}

/// Spawner for CoreExecutor - can be send to other threads for relaying work to this executor.
impl ThreadExecutorSpawner {
    /// Spawn a task with a shutdown guard. When dropped, the TaskShutdown struct
    /// will ensure that this task is joined on before allowing the tracking side
    /// thread to continue.
    ///
    /// The contract here is: if a persistent task is needed, be sure to check
    /// `shutdown.should_exit()`, allowing the tracking state to trigger a shutdown
    /// if required. Long-running tasks are joined on, and therefore will block at
    /// `unload` of a plugin.
    ///
    /// Note on safety:
    ///     If a plugin starts a long-lived task (i.e. one that allows the task to
    /// live longer than the enclosing scope), it can do so safely ONLY IF it is
    /// stopped before the plugin is unloaded. Think of it as: once the compiled
    /// code for a given task (i.e. the compiled plugin) has been unloaded, any
    /// further execution of the task will result in a memory violation/segfault.
    ///
    /// This is an example of the unsafe-ness of loading plugins in general, as the
    /// borrow-checker cannot know the lifetimes of things at compile time when we
    /// are loading types and dependent code at runtime.
    ///
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
        F: Future<Output = ()> + Send + Sync + 'static,
        T: FnOnce(channel::TaskWithShutdown) -> F,
    {
        let (killer, shutdown) = channel::TaskWithShutdown::new();
        self.fire(task_fn(shutdown));
        self.task_killers.push(killer);
    }

    pub fn spawn<T>(
        &mut self,
        task: Pin<Box<dyn Future<Output = T> + Send + Sync>>,
    ) -> impl Future<Output = Result<T, Closed>>
    where
        T: std::fmt::Debug + Send + Sync + 'static,
    {
        let (mut oneshot_tx, oneshot_rx) = async_oneshot::oneshot();
        self.tx
            .try_send(ExecutorTask::Task(
                async move {
                    oneshot_tx.send(task.await).unwrap();
                }
                .boxed(),
            ))
            .expect("unable to execute task");
        oneshot_rx
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

impl Drop for ThreadExecutorSpawner {
    fn drop(&mut self) {
        self.block_and_kill_tasks();
    }
}

#[cfg(test)]
mod tests {

    use std::pin::Pin;

    use crate::channel::Bichannel;

    use super::*;
    use async_executor::LocalExecutor;
    use futures_lite::{future, FutureExt};

    #[test]
    fn test_bichannel() {
        let exec = CoreAffinityExecutor::new(2);
        let left_spawner = &mut exec.spawners()[0];
        let right_spawner = &mut exec.spawners()[1];

        let (left, right): (Bichannel<(), i16>, Bichannel<i16, ()>) = Bichannel::bounded(1);

        let left_task = left_spawner.spawn(Box::pin(async move {
            left.send(()).await.unwrap();
            left.recv().await.unwrap()
        }));
        let right_task = right_spawner.spawn(Box::pin(async move {
            right.recv().await.unwrap();
            right.send(42).await.unwrap()
        }));
        let (left_result, right_result) =
            future::block_on(futures_util::future::join(left_task, right_task));
        assert_eq!(left_result, Ok(42));
        assert_eq!(right_result, Ok(()));
    }

    #[test]
    fn test_core_executor() {
        let exec = CoreAffinityExecutor::new(2);
        let spawner = &mut exec.spawners()[0];
        let answer_rx = spawner.spawn(Box::pin(async { 42 }));
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
                        println!("thing {} x {}", thing, x);
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
}
