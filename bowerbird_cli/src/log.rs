use chrono::Local;
use colored::Colorize;
use log4rs::{
    append::console::{ConsoleAppender, Target},
    config::{Appender, Config, Logger, Root},
    encode::Encode,
};
use std::env::var;
#[derive(Debug)]
struct ConsoleEncoder;

impl Encode for ConsoleEncoder {
    fn encode(
        &self,
        w: &mut dyn log4rs::encode::Write,
        record: &log::Record,
    ) -> anyhow::Result<()> {
        use log::Level::*;
        let level = record.level();
        let msg = record.args().to_string();

        let date = Local::now()
            .format("%Y-%m-%d %H:%M:%S")
            .to_string()
            .bright_black();

        let (level, msg) = match level {
            Error => (level.as_str().bright_red(), msg.bright_red()),
            Warn => (level.as_str().bright_yellow(), msg.bright_yellow()),
            Info => (level.as_str().bright_green(), msg.as_str().into()),
            Debug => (level.as_str().bright_blue(), msg.as_str().into()),
            _ => (level.as_str().into(), msg.as_str().into()),
        };

        let module = record.module_path().unwrap_or_default().bright_black();

        write!(w, "\n{date} {module} [{level}] {msg}\n")?;

        Ok(())
    }
}

fn level_from_env(key: &str, default: log::LevelFilter) -> log::LevelFilter {
    var(key)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}

pub fn init_log4rs() -> anyhow::Result<()> {
    let console_level = level_from_env("BOWERBIRD_CONSOLE_LOG_LEVEL", log::LevelFilter::Info);
    let console_level_root =
        level_from_env("BOWERBIRD_CONSOLE_LOG_LEVEL_ALL", log::LevelFilter::Warn);
    let console_out = ConsoleAppender::builder()
        .encoder(Box::new(ConsoleEncoder))
        .target(Target::Stderr)
        .build();
    let config = Config::builder()
        .appender(Appender::builder().build("console", Box::new(console_out)))
        .logger(Logger::builder().build("bowerbird_cli", console_level))
        .logger(Logger::builder().build("bowerbird_core", console_level))
        .logger(Logger::builder().build("bowerbird_server", console_level))
        .logger(Logger::builder().build("bowerbird_pixiv", console_level))
        .logger(Logger::builder().build("bowerbird_utils", console_level))
        .build(
            Root::builder()
                .appender("console")
                .build(console_level_root),
        )?;
    log4rs::init_config(config)?;
    Ok(())
}
