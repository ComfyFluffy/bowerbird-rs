use std::collections::BTreeMap;

use super::*;

#[derive(Clone, Default, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct PixivUser {
    pub is_followed: bool,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_following: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_illust_series: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_illusts: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_manga: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_novel_series: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_novels: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_public_bookmarks: Option<i64>,
}

#[derive(Clone, Default, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct PixivUserHistory {
    pub name: String,
    pub account: String,
    pub is_premium: bool,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub birth: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gender: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub twitter_account: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub web_page: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace: Option<BTreeMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_image_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
}

#[derive(Clone, Default, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct PixivWorks {
    pub total_bookmarks: i64,
    pub total_view: i64,
    pub is_bookmarked: bool,
}

#[derive(Clone, Default, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct PixivIllustHistory {
    pub illust_type: String, // TODO: use enum type
    pub caption_html: String,
    pub title: String,
    pub image_urls: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<DateTime>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ugoira_delay: Option<Vec<i32>>,
}

#[derive(Clone, Default, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct PixivNovelHistory {
    pub caption_html: String,
    pub title: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover_image_url: Option<String>,
    pub image_urls: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<DateTime>,
}
