use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

pub mod pixiv;

#[derive(Clone, Default, Debug, Deserialize, Serialize, PartialEq, Eq, FromRow)]
pub struct Tag {
    pub id: i64,
    pub alias: Vec<String>,
}

#[derive(Clone, Default, Debug, Deserialize, Serialize, PartialEq, Eq, FromRow)]
pub struct Item<E, H> {
    pub id: i64,
    #[sqlx(default)]
    pub parent_id: Option<i64>,
    pub source_id: Option<String>,
    pub source_inaccessible: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[sqlx(default)]
    pub inserted_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[sqlx(default)]
    pub updated_at: Option<DateTime<Utc>>,

    #[sqlx(default)]
    pub tag_ids: Vec<i64>,

    #[sqlx(flatten)]
    pub extension: E,
    #[sqlx(flatten)]
    pub history: History<H>,

    #[serde(skip)]
    pub _count: Option<i64>,
}

#[derive(Clone, Default, Debug, Deserialize, Serialize, PartialEq, Eq, FromRow)]
pub struct History<H> {
    pub history_id: i64,
    #[sqlx(default)]
    pub item_id: Option<i64>,
    #[sqlx(default)]
    pub updated_at: Option<DateTime<Utc>>,
    #[sqlx(flatten)]
    pub extension: H,
}

#[derive(Clone, Default, Debug, Deserialize, Serialize, PartialEq, Eq, FromRow)]
pub struct Media<E> {
    pub id: i64,
    pub url: Option<String>,
    pub size: Option<i32>,
    pub mime: Option<String>,
    pub local_path: Option<String>,

    #[sqlx(flatten)]
    pub extension: E,
}

#[derive(Clone, Default, Debug, Deserialize, Serialize, PartialEq, Eq, FromRow)]
pub struct Image {
    pub width: i32,
    pub height: i32,
    // #[sqlx(default)]
    // pub color: Vec<Hsv>,
}
