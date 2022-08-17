pub mod input;

use std::{error::Error, future::Future, pin::Pin};

use core_executor::TaskShutdownHandle;
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
}

impl InputState {
    pub fn new(execs: Vec<ThreadExecutorSpawner>) -> Self {
        Self {
            input_system: Default::default(),
            updates: 0,
            execs,
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

    pub fn print_msg(&self, m: &str) {
        println!("{}", m);
    }

    pub fn get_updates(&self) -> u64 {
        self.updates
    }
}


#[macro_export]
macro_rules! writeln {
    ($state:expr, $($args:tt)*) => {
        $state.print_msg(&::std::fmt::format(format_args!($($args)*)))
    };
}
