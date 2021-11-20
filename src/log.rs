use crate::config::LogLevel;
use colored::{ColoredString, Colorize};

pub static mut TARGET_LOG_LEVEL: LogLevel = LogLevel::Debug;

pub fn should_log(log_level: LogLevel) -> bool {
    log_level >= unsafe { TARGET_LOG_LEVEL }
}

macro_rules! debug {
	($($arg:tt)*) => {
		if $crate::log::should_log($crate::config::LogLevel::Debug) {
			use colored::Colorize;
			println!("\r{} [{}] {}",
			$crate::log::gray_datetime(),
			"DEBUG".bright_blue(),
			format!($($arg)*).bright_black());
		}
	};
}

macro_rules! info {
	($($arg:tt)*) => {
		if $crate::log::should_log($crate::config::LogLevel::Info) {
			use colored::Colorize;
			println!("\r{} [{}] {}",
			$crate::log::gray_datetime(),
			"INFO".bright_green(),
			format!($($arg)*));
		}
	};
}

macro_rules! warning {
	($($arg:tt)*) => {
		if $crate::log::should_log($crate::config::LogLevel::Warn) {
			use colored::Colorize;
			println!("\r{} [{}] {}",
			$crate::log::gray_datetime(),
			"WARNING".bright_yellow(),
			format!($($arg)*).bright_yellow());
		}
	};
}

macro_rules! error {
	($err:ident, $($arg:tt)*) => {
		if $crate::log::should_log($crate::config::LogLevel::Debug) {
			use colored::Colorize;
			println!("\r{} [{}] {}: {}{}",
			$crate::log::gray_datetime(),
			"ERROR".bright_red(),
			format!($($arg)*).bright_red(),
			$err,
			$err.backtrace()
				.map_or(" <no backtrace>".to_string(), |b| "\n".to_string() + &b.to_string()));
		}
	};
	($err:ident) => {
		if $crate::log::should_log($crate::config::LogLevel::Debug) {
			use colored::Colorize;
			println!("\r{} [{}] {}{}",
			$crate::log::gray_datetime(),
			"ERROR".bright_red(),
			$err,
			$err.backtrace()
				.map_or(" <no backtrace>".to_string(), |b| "\n".to_string() + &b.to_string()));
		}
	};
}
pub(crate) use {debug, error, info, warning};

pub fn gray_datetime() -> ColoredString {
    chrono::Local::now()
        .format("%m-%d %T")
        .to_string()
        .bright_black()
}
