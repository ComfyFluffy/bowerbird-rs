use chrono::{DateTime, Utc};

use crate::model::Rgb;

#[derive(Debug, Default, Clone)]
pub struct PixivQuery {
    pub tag_or: bool,
    pub tag: Vec<String>,
    pub search: Vec<String>, // Search in title and caption
    pub date_range: (Option<DateTime<Utc>>, Option<DateTime<Utc>>),
    pub colors: Vec<Rgb>,
    pub color_diff: i16,
}

impl PixivQuery {
    fn build(&self) -> bson::Document {
        let mut doc = bson::Document::new();
        if !self.tag.is_empty() {
            // doc.insert(key, val)
        }
        doc
    }
}
