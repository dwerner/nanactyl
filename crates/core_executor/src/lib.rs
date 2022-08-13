use std::{future::Future, pin::Pin, thread::JoinHandle};

use futures_channel::oneshot::Canceled;
use futures_lite::{future, FutureExt, StreamExt};

pub struct CoreExecutor {
    exec_thread_jh: Option<JoinHandle<()>>,
    tx: futures_channel::mpsc::Sender<ExecutorTask>,
}

impl Drop for CoreExecutor {
    fn drop(&mut self) {
        if let Some(thread_handle) = self.exec_thread_jh.take() {
            self.tx.try_send(ExecutorTask::Exit).unwrap();
            thread_handle.join().unwrap();
        }
    }
}

#[derive(Clone)]
pub struct CoreExecutorSpawner {
    tx: futures_channel::mpsc::Sender<ExecutorTask>,
}
enum ExecutorTask {
    Exit,
    Task(PinnedTask),
}

type PinnedTask = Pin<Box<dyn Future<Output = ()> + Send>>;

// Must be owned by a single thread
impl CoreExecutor {
    pub fn new() -> (CoreExecutorSpawner, Self) {
        let (tx, mut rx) = futures_channel::mpsc::channel::<ExecutorTask>(100);
        let exec_thread_jh = std::thread::spawn(move || {
            let local_exec = smol::LocalExecutor::new();
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
        (
            CoreExecutorSpawner { tx: tx.clone() },
            CoreExecutor {
                exec_thread_jh: Some(exec_thread_jh),
                tx,
            },
        )
    }
}

/// Spawner for CoreExecutor - can be send to other threads for relaying work to this executor.
impl CoreExecutorSpawner {
    pub fn spawn<'a, T>(
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
}

#[cfg(test)]
mod tests {

    use std::pin::Pin;

    use super::*;
    use futures_lite::{future, FutureExt};

    #[test]
    fn test_core_executor() {
        let (mut spawner, _exec) = CoreExecutor::new();
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
            let local_exec = smol::LocalExecutor::new();
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
