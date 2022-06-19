use chrono::Local;
use colored::Colorize;
use log4rs::{
    append::console::ConsoleAppender,
    config::{Appender, Config, Logger, Root},
    encode::Encode,
};
#[derive(Debug)]
struct Encoder;

impl Encode for Encoder {
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
            Warn => (
                level.as_str().bright_yellow(),
                msg.to_string().bright_yellow(),
            ),
            Info => (level.as_str().bright_green(), msg.as_str().into()),
            Debug => (level.as_str().bright_blue(), msg.as_str().into()),
            _ => (level.as_str().into(), msg.as_str().into()),
        };

        write!(w, "\n{date} [{level}] {msg}\n")?;

        Ok(())
    }
}

pub fn init_log4rs() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let console_out = ConsoleAppender::builder()
        .encoder(Box::new(Encoder))
        .build();
    let config = Config::builder()
        .appender(Appender::builder().build("console", Box::new(console_out)))
        .logger(Logger::builder().build("bowerbird", log::LevelFilter::Debug))
        .build(
            Root::builder()
                .appender("console")
                .build(log::LevelFilter::Warn),
        )?;
    log4rs::init_config(config)?;
    Ok(())
}
