use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

// Define LogLevel enum
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl FromStr for LogLevel {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "error" => Ok(LogLevel::Error),
            "warn" => Ok(LogLevel::Warn),
            "info" => Ok(LogLevel::Info),
            "debug" => Ok(LogLevel::Debug),
            "trace" => Ok(LogLevel::Trace),
            _ => Err(format!("Unknown log level: {}", s)),
        }
    }
}

impl fmt::Display for LogLevel {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            LogLevel::Error => write!(f, "E"),
            LogLevel::Warn => write!(f, "W"),
            LogLevel::Info => write!(f, "I"),
            LogLevel::Debug => write!(f, "D"),
            LogLevel::Trace => write!(f, "T"),
        }
    }
}

impl LogLevel {
    pub fn logger(self) -> Logger {
        Logger::new(self)
    }
}

/// Logger that logs to the console, implemented because the log crate doesn't
/// support resetting the log global variables it uses.

#[derive(Debug)]
pub struct Logger {
    pub level: LogLevel,
    path: Vec<String>,
}

impl Logger {
    pub fn new(level: LogLevel) -> Self {
        Logger {
            level,
            path: Vec::new(),
        }
    }

    pub fn log(&self, level: LogLevel, args: fmt::Arguments<'_>) {
        if level <= self.level {
            print!("{}(", level);
            for (i, item) in self.path.iter().enumerate() {
                print!("{}{item}", if i > 0 { "." } else { "" }, item = item);
            }
            println!("): {}", args);
        }
    }

    /// Create a new logger with the given name as a sub-logger of this one.
    pub fn sub(&self, name: &str) -> Self {
        let mut path = self.path.clone();
        path.push(name.to_string());
        Logger {
            level: self.level.clone(),
            path,
        }
    }

    // Convenience methods for each log level
    pub fn error(&self, args: fmt::Arguments<'_>) {
        self.log(LogLevel::Error, args);
    }

    pub fn warn(&self, args: fmt::Arguments<'_>) {
        self.log(LogLevel::Warn, args);
    }

    pub fn info(&self, args: fmt::Arguments<'_>) {
        self.log(LogLevel::Info, args);
    }

    pub fn debug(&self, args: fmt::Arguments<'_>) {
        self.log(LogLevel::Debug, args);
    }

    pub fn trace(&self, args: fmt::Arguments<'_>) {
        self.log(LogLevel::Trace, args);
    }
}

// Define log level macros
#[macro_export]
macro_rules! error {
    ($logger:expr, $($arg:tt)*) => {
        $logger.error(format_args!($($arg)*));
    };
}

#[macro_export]
macro_rules! warn {
    ($logger:expr, $($arg:tt)*) => {
        $logger.warn(format_args!($($arg)*));
    };
}

#[macro_export]
macro_rules! info {
    ($logger:expr, $($arg:tt)*) => {
        $logger.info(format_args!($($arg)*));
    };
}

#[macro_export]
macro_rules! debug {
    ($logger:expr, $($arg:tt)*) => {
        if $logger.level <= $crate::LogLevel::Debug {
            $logger.debug(format_args!($($arg)*));
        }
    };
}

#[macro_export]
macro_rules! trace {
    ($logger:expr, $($arg:tt)*) => {
        $logger.trace(format_args!($($arg)*));
    };
}
