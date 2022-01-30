use std::path::PathBuf;

use bson::from_bson;

use crate::model::{
    pixiv::{IllustHistory, PixivIllust},
    History, ImageMedia, LocalMedia, Tag,
};

enum InnerNode {
    Dir { children: Vec<Node> },
    File { path: PathBuf },
}

struct Node {
    display_name: String,
    inner: InnerNode,
}

enum SortBy {
    Id,
    AuthorId,
    Bookmarks,
    Views,
}

impl Default for SortBy {
    fn default() -> Self {
        Self::Id
    }
}

impl Node {
    fn from_pixiv(
        base_dir: PathBuf,
        mut illust: PixivIllust,
        sord_by: Option<SortBy>,
    ) -> Option<Self> {
        let history = illust.history.pop()?.extension?;
        let mut display_name = String::with_capacity(64);

        if let Some(sort_by) = sord_by {
            let key = r#"match sort_by {
                SortBy::AuthorId => illust
                    .parent_id
                    .map_or_else(|| "".to_string(), |o| o.to_hex()),
            };"#
            .to_string();
            display_name += &key;
            display_name.push('_');
        }

        display_name += &illust
            .source_id
            .unwrap_or_else(|| illust._id.unwrap_or_default().to_hex());
        display_name.push('_');

        display_name += &history.title.replace("\n", " ");
        display_name.push('_');

        if let Some(tags) = illust.other_fields.remove("tags") {
            let tags: Vec<Tag> = from_bson(tags).unwrap();
            for (i, t) in tags.into_iter().take(3).enumerate() {
                if let Some(ts) = t.alias.first() {
                    if i > 0 {
                        display_name.push('_')
                    }
                    display_name += ts;
                }
            }
        }

        let media: Vec<LocalMedia<ImageMedia>> =
            from_bson(illust.other_fields.remove("media")?).unwrap();
        let mut path = base_dir;
        // path.push(media.local_path);
        Some(Self {
            display_name,
            inner: InnerNode::File { path },
        })
    }
}
