pub mod input;

use std::{error::Error, future::Future, pin::Pin};

pub use core_executor::ThreadExecutorSpawner;
pub use futures_lite;

use futures_lite::future;
pub use log;

use log::{Level, LevelFilter, Metadata, Record};

pub struct SimpleLogger;
static LOGGER: SimpleLogger = SimpleLogger;

pub fn init_logger() {
    log::set_logger(&LOGGER).unwrap();
    log::set_max_level(LevelFilter::Info);
}

impl log::Log for SimpleLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Info
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            println!("{} - {}", record.level(), record.args());
        }
    }

    fn flush(&self) {}
}

pub struct InputState {
    pub input_system: Option<Box<dyn input::InputEventSource>>,
    updates: u64,
    pub execs: Vec<ThreadExecutorSpawner>,
    task_killers: Vec<(async_channel::Sender<()>, async_channel::Receiver<()>)>,
}

impl InputState {
    pub fn new(execs: Vec<ThreadExecutorSpawner>) -> Self {
        Self {
            input_system: Default::default(),
            updates: 0,
            execs,
            task_killers: Vec::new(),
        }
    }

    // TODO: how should plugins know what core to send work to? Round robin here?
    pub fn spawner(&mut self) -> ThreadExecutorSpawner {
        self.execs[0].clone()
    }

    pub fn spawner_for_core(&self, id: usize) -> Option<ThreadExecutorSpawner> {
        self.execs
            .iter()
            .find(|ThreadExecutorSpawner { core_id, .. }| *core_id == id)
            .cloned()
    }

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
    //    state.spawn_with_shutdown(|shutdown| {
    //    Box::pin(async move {
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
    //    })
    //  });
    //```
    pub fn spawn_with_shutdown(
        &mut self,
        task_fn: impl FnOnce(TaskShutdown) -> Pin<Box<dyn Future<Output = ()> + Send + Sync>>,
    ) {
        let (kill_sender, should_break) = async_channel::unbounded::<()>();
        let (confirm_sender, confirm_ended) = async_channel::unbounded::<()>();
        let shutdown = TaskShutdown {
            should_break,
            confirm_sender,
        };
        self.spawner().fire(task_fn(shutdown));
        self.track_task(kill_sender, confirm_ended);
    }

    pub fn print_msg(&self, m: &str) {
        println!("{}", m);
    }

    pub fn get_updates(&self) -> u64 {
        self.updates
    }

    // todo move fire in here
    fn track_task(&mut self, tx: async_channel::Sender<()>, confirm: async_channel::Receiver<()>) {
        self.task_killers.push((tx, confirm));
    }

    fn block_and_kill_tasks(&mut self) {
        writeln!(
            self,
            "killing {} long-running tasks...",
            self.task_killers.len()
        );
        for (killer, confirm) in self.task_killers.drain(..) {
            killer.send_blocking(()).expect("unable to kill task");
            future::block_on(async move {
                confirm.recv().await.expect("error during confirm");
            });
        }
    }
}

impl Drop for InputState {
    fn drop(&mut self) {
        self.block_and_kill_tasks();
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
    kill_send: async_channel::Sender<()>,
    kill_confirmation: async_channel::Receiver<()>,
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

#[macro_export]
macro_rules! writeln {
    ($state:expr, $($args:tt)*) => {
        $state.print_msg(&::std::fmt::format(format_args!($($args)*)))
    };
}
