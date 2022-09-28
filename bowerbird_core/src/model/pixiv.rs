use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use super::Item;

#[derive(Clone, Default, Debug, Deserialize, Serialize, PartialEq, Eq, FromRow)]
pub struct User {
    pub is_followed: bool,
    pub total_following: Option<i32>,
    pub total_illust_series: Option<i32>,
    pub total_illusts: Option<i32>,
    pub total_manga: Option<i32>,
    pub total_novel_series: Option<i32>,
    pub total_novels: Option<i32>,
    pub total_public_bookmarks: Option<i32>,
}

#[derive(Clone, Default, Debug, Deserialize, Serialize, PartialEq, Eq, FromRow)]
pub struct Works {
    pub total_bookmarks: i32,
    pub total_view: i32,
    pub is_bookmarked: bool,
}

#[derive(Clone, Default, Debug, Deserialize, Serialize, PartialEq, Eq, FromRow)]
pub struct IllustHistory {
    pub illust_type: String,
    pub caption_html: String,
    pub title: String,
    pub date: Option<DateTime<Utc>>,
    pub image_paths: Option<Vec<Option<String>>>,
    // pub ugoira_frame_duration: Option<Vec<i32>>,
}

pub type PixivIllust = Item<Works, IllustHistory>;
