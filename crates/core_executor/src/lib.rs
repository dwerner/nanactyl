use std::{future::Future, pin::Pin, thread::JoinHandle};

use async_executor::LocalExecutor;
use futures_channel::oneshot::Canceled;
use futures_lite::{future, FutureExt, StreamExt};

pub struct ThreadExecutor {
    _core_id: usize,
    spawner: ThreadExecutorSpawner,
    exec_thread_jh: Option<JoinHandle<()>>,
}

impl ThreadExecutor {
    pub fn new(core_id: usize) -> Self {
        let (tx, mut rx) = futures_channel::mpsc::channel::<ExecutorTask>(100);
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
            spawner: ThreadExecutorSpawner { core_id, tx },
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

#[derive(Clone)]
pub struct ThreadExecutorSpawner {
    pub core_id: usize,
    tx: futures_channel::mpsc::Sender<ExecutorTask>,
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
}

/// Spawner for CoreExecutor - can be send to other threads for relaying work to this executor.
impl ThreadExecutorSpawner {
    pub fn spawn<T>(
        &mut self,
        task: impl Future<Output = T> + Send + 'static,
    ) -> impl Future<Output = Result<T, Canceled>>
    where
        T: std::fmt::Debug + Send + 'static,
    {
        let (oneshot_tx, oneshot_rx) = futures_channel::oneshot::channel();
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
}

#[cfg(test)]
mod tests {

    use std::pin::Pin;

    use super::*;
    use async_executor::LocalExecutor;
    use futures_lite::{future, FutureExt};

    #[test]
    fn test_core_executor() {
        let exec = CoreAffinityExecutor::new(2);
        let spawner = &mut exec.spawners()[0];
        let answer_rx = spawner.spawn(async { 42 });
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
            let (task_tx, task_rx) = futures_channel::oneshot::channel();
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
