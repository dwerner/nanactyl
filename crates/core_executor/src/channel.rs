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

/// A bi-directional channel.
pub struct Bichannel<S, R> {
    send: async_channel::Sender<S>,
    recv: async_channel::Receiver<R>,
}

// Rather than derive clone here, implement it manually or we require that S and
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
    /// Simple bi-directional channel on top of async_channel.
    /// Send and receive can be different types.
    /// send from left(send) -> right(recv) -> right(send) -> left(recv)
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

    pub async fn send(&self, msg: S) -> Result<(), BichannelError<S>> {
        self.send
            .send(msg)
            .await
            .map_err(BichannelError::ChannelSendError)
    }

    pub async fn recv(&self) -> Result<R, async_channel::RecvError> {
        self.recv.recv().await
    }

    pub fn send_blocking(&self, msg: S) -> Result<(), async_channel::SendError<S>> {
        self.send.send_blocking(msg)
    }

    pub fn recv_blocking(&self) -> Result<R, async_channel::RecvError> {
        self.recv.recv_blocking()
    }
}

/// Bidirectional single-shot channel built on top of async_oneshot.
/// Send and receive can be different types.
pub struct Hookshot<S, R> {
    send: async_oneshot::Sender<S>,
    recv: Option<async_oneshot::Receiver<R>>,
}

impl<S, R> Hookshot<S, R> {
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

    pub fn send_blocking(&mut self, msg: S) -> Result<(), HookshotError> {
        self.send.send(msg).map_err(HookshotError::OneshotClosed)
    }

    pub fn try_recv(&mut self) -> Result<R, HookshotError> {
        match self.recv.take() {
            Some(recv) => match recv.try_recv() {
                Ok(r) => return Ok(r),
                Err(async_oneshot::TryRecvError::Empty(receiver)) => {
                    self.recv = Some(receiver);
                    return Err(HookshotError::OneshotEmpty);
                }
                Err(async_oneshot::TryRecvError::Closed) => {
                    return Err(HookshotError::OneshotTryRecvClosed);
                }
            },
            None => unreachable!("should never be reached"),
        }
    }

    pub async fn recv(self) -> Result<R, HookshotError> {
        self.recv
            .expect("unreachable - hookshot receiver None")
            .await
            .map_err(HookshotError::OneshotClosed)
    }

    pub fn recv_blocking(self) -> Result<R, HookshotError> {
        futures_lite::future::block_on(self.recv())
    }
}

pub struct TaskShutdownHandle {
    kill_send: Hookshot<(), ()>,
}

pub struct TaskWithShutdown {
    kill_confirm: Hookshot<(), ()>,
}

impl TaskWithShutdown {
    pub fn new() -> (TaskShutdownHandle, TaskWithShutdown) {
        let (kill_send, kill_confirm) = Hookshot::new();
        let handle = TaskShutdownHandle { kill_send };
        let shutdown = TaskWithShutdown { kill_confirm };
        (handle, shutdown)
    }

    pub fn should_exit(&mut self) -> bool {
        if let Ok(()) = self.kill_confirm.try_recv() {
            return true;
        }
        false
    }
}

impl TaskShutdownHandle {
    pub async fn shutdown(mut self) -> Result<(), HookshotError> {
        self.kill_send.send_blocking(())?;
        self.kill_send.recv().await?;
        Ok(())
    }
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
