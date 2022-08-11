pub mod input;

pub use log;
use log::{Level, LevelFilter, Metadata, Record};
pub use sdl2;

use sdl2::Sdl;

pub struct SimpleLogger;
static LOGGER: SimpleLogger = SimpleLogger;

pub fn init_logger() -> &'static SimpleLogger {
    log::set_logger(&LOGGER).unwrap();
    log::set_max_level(LevelFilter::Info);
    &LOGGER
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

pub struct State {
    pub input_system: Option<Box<dyn input::InputEventSource>>,
    pub sdl_context: Sdl,
    pub logger: &'static SimpleLogger,
    updates: u64,
}

impl State {
    pub fn new(sdl_context: Sdl, logger: &'static SimpleLogger) -> Self {
        Self {
            sdl_context,
            input_system: Default::default(),
            logger,
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
