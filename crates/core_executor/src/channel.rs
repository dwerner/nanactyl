use std::error::Error;


pub struct Bichannel<S, R> {
    send: async_channel::Sender<S>,
    recv: async_channel::Receiver<R>,
}

impl<S, R> Bichannel<S, R> {
    /// Simple bi-directional channel on top of async_channel.
    /// Send and receive can be different types.
    pub fn bounded(cap: usize) -> (Bichannel<R, S>, Bichannel<S, R>) {
        let (left_send, left_recv) = async_channel::bounded::<S>(cap);
        let (right_send, right_recv) = async_channel::bounded::<R>(cap);
        (
            Bichannel {
                recv: left_recv,
                send: right_send,
            },
            Bichannel {
                recv: right_recv,
                send: left_send,
            },
        )
    }

    pub async fn send(&self, msg: S) -> Result<(), async_channel::SendError<S>> {
        self.send.send(msg).await
    }

    pub async fn recv(&self) -> Result<R, async_channel::RecvError> {
        self.recv.recv().await
    }

    pub fn send_blocking(&self, msg: S) -> Result<(), async_channel::SendError<S>> {
        self.send.send_blocking(msg)
    }

    pub async fn recv_blocking(&self) -> Result<R, async_channel::RecvError> {
        self.recv.recv_blocking()
    }
}

/// Simple bi-directional oneshot channel on top of async_oneshot.
/// Send and receive can be different types.
pub struct PingPong<S, R> {
    send: async_oneshot::Sender<S>,
    recv: async_oneshot::Receiver<R>,
}

impl<S, R> PingPong<S, R> {
    pub fn new() -> (PingPong<R, S>, PingPong<S, R>) {
        let (left_send, left_recv) = async_oneshot::oneshot::<S>();
        let (right_send, right_recv) = async_oneshot::oneshot::<R>();
        (
            PingPong {
                recv: left_recv,
                send: right_send,
            },
            PingPong {
                recv: right_recv,
                send: left_send,
            },
        )
    }

    pub fn send(&mut self, msg: S) -> Result<(), async_oneshot::Closed> {
        self.send.send(msg)
    }

    pub async fn recv(self) -> Result<R, async_oneshot::TryRecvError<R>> {
        self.recv.try_recv()
    }
}

pub struct TaskShutdown {
    should_break: async_channel::Receiver<()>,
    confirm_sender: async_channel::Sender<()>,
}

impl TaskShutdown {
    pub fn new() -> (TaskShutdownHandle, TaskShutdown) {
        let (kill_send, should_break) = async_channel::bounded(1);
        let (confirm_sender, kill_confirmation) = async_channel::bounded(1);
        let handle = TaskShutdownHandle {
            kill_send,
            kill_confirmation,
        };
        let shutdown = TaskShutdown {
            confirm_sender,
            should_break,
        };
        (handle, shutdown)
    }
    pub fn should_exit(&self) -> bool {
        if let Ok(()) = self.should_break.try_recv() {
            return true;
        }
        false
    }
}

pub struct TaskShutdownHandle {
    pub kill_send: async_channel::Sender<()>,
    pub kill_confirmation: async_channel::Receiver<()>,
}

impl TaskShutdownHandle {
    pub fn shutdown_blocking(&self) -> Result<(), Box<dyn Error>> {
        self.kill_send.send_blocking(())?;
        self.kill_confirmation.recv_blocking()?;
        Ok(())
    }
    pub async fn shutdown(&self) -> Result<(), Box<dyn Error>> {
        self.kill_send.send(()).await?;
        self.kill_confirmation.recv().await?;
        Ok(())
    }
}

impl Drop for TaskShutdown {
    fn drop(&mut self) {
        self.confirm_sender
            .send_blocking(())
            .expect("unable to send shutdown confirmation")
    }
}