pub mod pixiv;

use mongodb::bson::{oid::ObjectId, DateTime};
use serde::{Deserialize, Serialize};

#[derive(Clone, Default, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Item<E, H> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _id: Option<ObjectId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<ObjectId>,
    pub tag_ids: Vec<ObjectId>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
    pub source_inaccessible: bool,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_modified: Option<DateTime>,

    pub history: Vec<History<H>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub extension: Option<E>,
}

#[derive(Clone, Default, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct History<H> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_modified: Option<DateTime>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extension: Option<H>,
}

#[derive(Clone, Default, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct LocalMedia<T> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _id: Option<ObjectId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    pub size: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime: Option<String>,
    pub local_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extension: Option<T>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct RGB(pub i16, pub i16, pub i16);

#[derive(Clone, Default, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ImageMedia {
    pub width: i32,
    pub height: i32,
    pub palette_rgb: Vec<RGB>,
}

#[derive(Clone, Default, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Tag {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _id: Option<ObjectId>,
    pub alias: Vec<String>,
    pub protected: bool,
}

#[derive(Clone, Default, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Collection {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _id: Option<ObjectId>,
    pub name: String,
    pub item_ids: Vec<ObjectId>,
}
