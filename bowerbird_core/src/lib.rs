use std::time::Instant;

use log::debug;
use sqlx::{migrate::MigrateError, PgPool};

pub mod config;

pub async fn migrate(db: &PgPool) -> Result<(), MigrateError> {
    debug!("migration started");
    let t = Instant::now();
    sqlx::migrate!().run(db).await?;
    debug!("migration finished: {:?}", t.elapsed());
    Ok(())
}
