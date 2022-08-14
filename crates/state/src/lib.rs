pub mod input;

use std::{future::Future, pin::Pin};

pub use core_executor::ThreadExecutorSpawner;
pub use futures_lite;

use futures_lite::future;
use input::InputEvent;
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
    pub fn track_task(
        &mut self,
        tx: async_channel::Sender<()>,
        confirm: async_channel::Receiver<()>,
    ) {
        self.task_killers.push((tx, confirm));
    }

    pub fn block_and_kill_tasks(&mut self) {
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
    pub fn should_exit(&self) -> bool {
        if let Ok(()) = self.should_break.try_recv() {
            return true;
        }
        false
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
