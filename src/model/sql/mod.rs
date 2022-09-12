use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

pub mod pixiv;

#[derive(Clone, Default, Debug, Deserialize, Serialize, PartialEq, FromRow)]
pub struct Item<E> {
    pub id: i32,
    #[sqlx(default)]
    pub parent_id: Option<i32>,
    pub source_id: Option<String>,
    pub source_inaccessible: bool,
    pub inserted_at: DateTime<Utc>,
    pub last_modified: Option<DateTime<Utc>>,
    // pub history: Vec<History<H>>,
    #[sqlx(flatten)]
    pub extension: E,
    // pub history: PhantomData<H>,
}

#[derive(Clone, Default, Debug, Deserialize, Serialize, PartialEq, Eq, FromRow)]
pub struct History<H> {
    pub id: i32,
    pub item_id: i32,
    pub last_modified: Option<DateTime<Utc>>,
    #[sqlx(flatten)]
    pub extension: H,
}
