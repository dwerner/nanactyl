use std::error::Error;
use std::fmt::Debug;

/// Errors possible from use of Bichannel.
#[derive(thiserror::Error, Debug)]
pub enum BichannelError<S> {
    #[error("Channel TryRecvError {0:?}")]
    ChannelTryRecvErr(async_channel::TryRecvError),

    #[error("Channel SendError {0:?}")]
    ChannelSendError(async_channel::SendError<S>),

    #[error("Channel RecvError {0:?}")]
    ChannelRecvError(async_channel::RecvError),
}

/// Errors possible from Hookshot.
#[derive(thiserror::Error, Debug)]
pub enum HookshotError {
    #[error("Oneshot closed {0:?}")]
    OneshotClosed(async_oneshot::Closed),

    #[error("Oneshot TryRecvError::Empty")]
    OneshotEmpty,

    #[error("Oneshot TryRecvError::Closed")]
    OneshotTryRecvClosed,
}
/// A bi-directional channel built on top of `async_channel`.
/// Each bichannel has a send channel and a receive channel.
/// The send channel sends values to the receive channel of the other Bichannel.
/// The receive channel receives values from the send channel of the other
/// Bichannel. Each channel can only be accessed through the Bichannel.
///
/// # Example
///
/// ```
/// use core_executor::channel::Bichannel;
///
/// async fn send_and_receive() {
///     let (mut left, mut right) = Bichannel::bounded(10);
///     left.send("Hello, world!").await.unwrap();
///     let message = right.recv().await.unwrap();
///     assert_eq!(message, "Hello, world!");
///     right.send("Oh, hello.").await.unwrap();
///     let response = left.recv().await.unwrap();
///     assert_eq!(message, "Oh, hello.");
/// }
/// ```
pub struct Bichannel<S, R> {
    send: async_channel::Sender<S>,
    recv: async_channel::Receiver<R>,
}

// Rather than derive clone here, implement it manually or we require that S and
// T are Clone.  This also makes the clone function more robust, covering more
// edge cases and handling errors.
// R: Clone as well.
impl<S, R> Clone for Bichannel<S, R> {
    fn clone(&self) -> Self {
        Self {
            send: self.send.clone(),
            recv: self.recv.clone(),
        }
    }
}

impl<S, R> Bichannel<S, R> {
    /// Creates a new bi-directional channel with a specified buffer capacity.
    /// Send and receive can be different types.
    /// `send` from left(send) -> right(recv) -> right(send) -> left(recv)
    ///
    /// # Arguments
    ///
    /// * `cap` - The buffer capacity for the channel.
    ///
    /// # Returns
    ///
    /// A tuple of two `Bichannel`s, one for each end of the channel.
    pub fn bounded(cap: usize) -> (Bichannel<S, R>, Bichannel<R, S>) {
        let (left_send, left_recv) = async_channel::bounded::<S>(cap);
        let (right_send, right_recv) = async_channel::bounded::<R>(cap);
        (
            Bichannel {
                send: left_send,
                recv: right_recv,
            },
            Bichannel {
                recv: left_recv,
                send: right_send,
            },
        )
    }

    /// Sends a message on the channel asynchronously.
    ///
    /// # Arguments
    ///
    /// * `msg` - The message to send.
    ///
    /// # Returns
    ///
    /// A `Result` indicating success or an error variant from the
    /// `BichannelError` enum.
    pub async fn send(&self, msg: S) -> Result<(), BichannelError<S>> {
        self.send
            .send(msg)
            .await
            .map_err(BichannelError::ChannelSendError)
    }

    /// Receives a message from the channel asynchronously.
    ///
    /// # Returns
    ///
    /// A `Result` containing the received message or an error variant from the
    /// `async_channel::RecvError` enum.
    pub async fn recv(&self) -> Result<R, async_channel::RecvError> {
        self.recv.recv().await
    }

    /// Sends a message on the channel, blocking the current thread.
    ///
    /// # Arguments
    ///
    /// * `msg` - The message to send.
    ///
    /// # Returns
    ///
    /// A `Result` indicating success or an error variant from the
    /// `async_channel::SendError` enum.
    pub fn send_blocking(&self, msg: S) -> Result<(), async_channel::SendError<S>> {
        self.send.send_blocking(msg)
    }

    /// Receives a message from the channel, blocking the current thread.
    ///
    /// # Returns
    ///
    /// A `Result` containing the received message or an error variant from the
    /// `async_channel::RecvError` enum.
    pub fn recv_blocking(&self) -> Result<R, async_channel::RecvError> {
        self.recv.recv_blocking()
    }
}

/// A single-shot, bi-directional channel built on top of async_oneshot.
/// Send and receive can be different types.
pub struct Hookshot<S, R> {
    send: async_oneshot::Sender<S>,
    recv: Option<async_oneshot::Receiver<R>>,
}

impl<S, R> Hookshot<S, R> {
    /// Creates a new bi-directional single-shot channel.
    /// Send and receive can be different types.
    ///
    /// # Returns
    ///
    /// A tuple of two Hookshots, one for each end of the channel.
    pub fn new() -> (Hookshot<R, S>, Hookshot<S, R>) {
        let (left_send, left_recv) = async_oneshot::oneshot::<S>();
        let (right_send, right_recv) = async_oneshot::oneshot::<R>();
        (
            Hookshot {
                recv: Some(left_recv),
                send: right_send,
            },
            Hookshot {
                recv: Some(right_recv),
                send: left_send,
            },
        )
    }

    /// Sends a message on the channel, blocking the current thread.
    ///
    /// # Arguments
    ///
    /// * `msg` - The message to send.
    ///
    /// # Returns
    ///
    /// A `Result` indicating success or an error variant from the
    /// `HookshotError` enum.
    pub fn send_blocking(&mut self, msg: S) -> Result<(), HookshotError> {
        self.send.send(msg).map_err(HookshotError::OneshotClosed)
    }

    /// Attempts to receive a message from the channel without blocking.
    ///
    /// # Returns
    ///
    /// A `Result` containing the received message or an error variant from the
    /// `HookshotError` enum.
    pub fn try_recv(&mut self) -> Result<R, HookshotError> {
        match self.recv.take() {
            Some(recv) => match recv.try_recv() {
                Ok(r) => Ok(r),
                Err(async_oneshot::TryRecvError::Empty(receiver)) => {
                    self.recv = Some(receiver);
                    Err(HookshotError::OneshotEmpty)
                }
                Err(async_oneshot::TryRecvError::Closed) => {
                    Err(HookshotError::OneshotTryRecvClosed)
                }
            },
            None => unreachable!("should never be reached"),
        }
    }

    /// Receives a message from the channel asynchronously.
    ///
    /// # Returns
    ///
    /// A `Result` containing the received message or an error variant from the
    /// `HookshotError` enum.
    pub async fn recv(self) -> Result<R, HookshotError> {
        self.recv
            .expect("unreachable - hookshot receiver None")
            .await
            .map_err(HookshotError::OneshotClosed)
    }

    /// Receives a message from the channel, blocking the current thread.
    ///
    /// # Returns
    ///
    /// A `Result` containing the received message or an error variant from the
    /// `HookshotError` enum.
    pub fn recv_blocking(self) -> Result<R, HookshotError> {
        futures_lite::future::block_on(self.recv())
    }
}

/// A handle for gracefully shutting down a task.
pub struct TaskShutdownHandle {
    kill_send: Hookshot<(), ()>,
}

/// A task wrapper that supports graceful shutdown.
pub struct TaskWithShutdown {
    kill_confirm: Hookshot<(), ()>,
}

impl TaskWithShutdown {
    /// Creates a new TaskWithShutdown and its associated TaskShutdownHandle.
    ///
    /// # Returns
    ///
    /// A tuple containing the TaskShutdownHandle and the TaskWithShutdown.
    pub fn new() -> (TaskShutdownHandle, TaskWithShutdown) {
        let (kill_send, kill_confirm) = Hookshot::new();
        let handle = TaskShutdownHandle { kill_send };
        let shutdown = TaskWithShutdown { kill_confirm };
        (handle, shutdown)
    }

    /// Checks if the task should exit gracefully.
    ///
    /// # Returns
    ///
    /// A boolean indicating whether the task should exit.
    pub fn should_exit(&mut self) -> bool {
        if let Ok(()) = self.kill_confirm.try_recv() {
            return true;
        }
        false
    }
}

impl TaskShutdownHandle {
    /// Asynchronously shuts down the associated task.
    ///
    /// # Returns
    ///
    /// A Result indicating success or an error variant from the HookshotError
    /// enum.
    pub async fn shutdown(mut self) -> Result<(), HookshotError> {
        self.kill_send.send_blocking(())?;
        self.kill_send.recv().await?;
        Ok(())
    }

    /// Shuts down the associated task, blocking the current thread.
    ///
    /// # Returns
    ///
    /// A `Result` indicating success or an error variant from the `Box<dyn
    /// Error>` type.
    pub fn shutdown_blocking(mut self) -> Result<(), Box<dyn Error>> {
        self.kill_send.send_blocking(())?;
        self.kill_send.recv_blocking()?;
        Ok(())
    }
}

impl Drop for TaskWithShutdown {
    fn drop(&mut self) {
        self.kill_confirm
            .send_blocking(())
            .expect("unable to send shutdown confirmation")
    }
}

#[cfg(test)]
mod tests {
    use futures_lite::stream::{self, StreamExt};

    use super::*;
    use crate::ThreadPoolExecutor;

    #[smol_potat::test]
    async fn test_bichannel() {
        let (sender, receiver) = Bichannel::<i32, i32>::bounded(10);
        let mut executor = ThreadPoolExecutor::new(2);

        let send_task = async move {
            for i in 0..10 {
                sender.send(i).await.unwrap();
            }
        };

        let recv_task = async move {
            stream::unfold(
                receiver,
                |r| async move { Some((r.recv().await.unwrap(), r)) },
            )
            .take(10)
            .collect::<Vec<_>>()
            .await
        };

        executor.spawn_on_any(send_task).1.await.unwrap();
        let recv_handle = executor.spawn_on_any(recv_task).1;

        let received = recv_handle.await.unwrap();

        assert_eq!(received, (0..10).collect::<Vec<_>>());
    }

    #[smol_potat::test]
    async fn test_hookshot() {
        let (mut sender, receiver) = Hookshot::<&str, &str>::new();
        let mut executor = ThreadPoolExecutor::new(2);

        let send_task = async move { sender.send_blocking("Hello, world!").unwrap() };

        let recv_task = async move { receiver.recv().await.unwrap() };

        let (_core, send_handle) = executor.spawn_on_any(send_task);

        send_handle.await.unwrap();
        let (_core, received) = executor.spawn_on_any(recv_task);

        assert_eq!(received.await.unwrap(), "Hello, world!");
    }

    #[smol_potat::test]
    async fn test_task_with_shutdown() {
        let mut executor = ThreadPoolExecutor::new(2);

        let (shutdown_handle, mut task_with_shutdown) = TaskWithShutdown::new();

        let task = async move {
            while !task_with_shutdown.should_exit() {
                // Simulate some work
                smol::Timer::after(std::time::Duration::from_millis(10)).await;
            }
            "Task finished"
        };

        let (_core_id, task_handle) = executor.spawn_on_any(task);

        // Allow the task to run for a while before shutting it down
        // Initiate the shutdown process
        smol::Timer::after(std::time::Duration::from_secs(1)).await;

        shutdown_handle.shutdown().await.unwrap();

        // Wait for the task to finish and check the result
        let result = task_handle.await.unwrap();
        assert_eq!(result, "Task finished");
    }
}
