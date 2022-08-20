pub mod input;

pub use core_executor::ThreadExecutorSpawner;
pub use futures_lite;

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
}

impl InputState {
    pub fn new() -> Self {
        Self {
            input_system: Default::default(),
            updates: 0,
        }
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
