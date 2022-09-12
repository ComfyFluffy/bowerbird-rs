use sqlx::FromRow;

#[derive(FromRow, Debug, Clone, PartialEq, Eq)]
pub struct User {
    pub is_followed: bool,
    pub total_following: Option<i64>,
    pub total_illust_series: Option<i64>,
    pub total_illusts: Option<i64>,
    pub total_manga: Option<i64>,
    pub total_novel_series: Option<i64>,
    pub total_novels: Option<i64>,
    pub total_public_bookmarks: Option<i64>,
}

#[cfg(test)]
mod tests {

    use bson::doc;
    use dotenvy_macro::dotenv;
    use futures::TryStreamExt;
    use mongodb::Database;
    use sqlx::{MySql, Pool, QueryBuilder};

    use crate::model::{
        pixiv::{PixivIllust, PixivNovel, PixivUser},
        ImageMedia, LocalMedia, Tag,
    };

    async fn get_mongo() -> Database {
        mongodb::Client::with_options(
            mongodb::options::ClientOptions::parse(dotenv!("MONGODB_URL"))
                .await
                .unwrap(),
        )
        .unwrap()
        .database("bowerbird_rust")
    }

    async fn get_mysql() -> Pool<MySql> {
        sqlx::MySqlPool::connect(dotenv!("DATABASE_URL"))
            .await
            .unwrap()
    }

    async fn images_() {
        let mdb = get_mongo().await;
        let collection = mdb.collection::<LocalMedia<ImageMedia>>("pixiv_image");
        let old: Vec<_> = collection
            .find(None, None)
            .await
            .unwrap()
            .try_collect()
            .await
            .unwrap();

        let pool = get_mysql().await;
        let mut tr = pool.begin().await.unwrap();

        for img in old {
            let mut q = QueryBuilder::<MySql>::new(
                "INSERT INTO pixiv_media (url, size, mime, local_path, width, height) ",
            );
            q.push_values(Some(&img), |mut b, img| {
                b.push_bind(&img.url)
                    .push_bind(img.size)
                    .push_bind(&img.mime)
                    .push_bind(&img.local_path)
                    .push_bind(img.extension.as_ref().map(|v| v.width))
                    .push_bind(img.extension.as_ref().map(|v| v.height));
            });
            let media_id = q.build().execute(&mut tr).await.unwrap().last_insert_id();

            if let Some(ext) = img.extension {
                if !ext.palette_hsv.is_empty() {
                    let mut q = QueryBuilder::<MySql>::new(
                        "INSERT INTO pixiv_image_color (image_id, h, s, v) ",
                    );
                    q.push_values(ext.palette_hsv, |mut b, color| {
                        b.push_bind(media_id)
                            .push_bind(color.h)
                            .push_bind(color.s)
                            .push_bind(color.v);
                    });
                    q.build().execute(&mut tr).await.unwrap();
                }
            }
        }

        tr.commit().await.unwrap();
    }
    #[tokio::test]
    async fn images() {
        images_().await
    }

    async fn users_() {
        let mdb = get_mongo().await;
        let c_user = mdb.collection::<PixivUser>("pixiv_user");
        let old: Vec<_> = c_user
            .find(None, None)
            .await
            .unwrap()
            .try_collect()
            .await
            .unwrap();

        let pool = get_mysql().await;
        let mut tr = pool.begin().await.unwrap();
        for user in old {
            // println!("{:?}\n", user);
            let mut q = QueryBuilder::<MySql>::new(
            "INSERT INTO pixiv_user (source_id, source_inaccessible, last_modified, is_followed, total_following, total_illust_series, total_illusts, total_manga, total_novel_series, total_novels, total_public_bookmarks) "
            );
            q.push_values([&user], |mut b, user| {
                b.push_bind(&user.source_id)
                    .push_bind(user.source_inaccessible)
                    .push_bind(user.last_modified.map(|dt| dt.to_chrono()))
                    .push_bind(user.extension.as_ref().map(|v| v.is_followed))
                    .push_bind(user.extension.as_ref().map(|v| v.total_following))
                    .push_bind(user.extension.as_ref().map(|v| v.total_illust_series))
                    .push_bind(user.extension.as_ref().map(|v| v.total_illusts))
                    .push_bind(user.extension.as_ref().map(|v| v.total_manga))
                    .push_bind(user.extension.as_ref().map(|v| v.total_novel_series))
                    .push_bind(user.extension.as_ref().map(|v| v.total_novels))
                    .push_bind(user.extension.as_ref().map(|v| v.total_public_bookmarks));
            });

            let query = q.build();
            let item_id = query.execute(&mut tr).await.unwrap().last_insert_id();

            let mut query_builder = QueryBuilder::<MySql>::new(
                "insert into pixiv_user_history (item_id, workspace_image_id, background_id, avatar_id, last_modified, birth, region,
                    gender, comment, twitter_account, web_page, workspace)",
            );
            query_builder.push_values(&user.history, |mut b, h| {
                let ext = h.extension.as_ref();
                let select_id = "(SELECT id FROM pixiv_media where url = ";
                b.push_bind(item_id)
                    .push(select_id)
                    .push_bind_unseparated(ext.map(|v| &v.workspace_image_url))
                    .push_unseparated(")")
                    .push(select_id)
                    .push_bind_unseparated(ext.map(|v| &v.background_url))
                    .push_unseparated(")")
                    .push(select_id)
                    .push_bind_unseparated(ext.map(|v| &v.avatar_url))
                    .push_unseparated(")")
                    .push_bind(h.last_modified.map(|dt| dt.to_chrono()))
                    .push_bind(ext.map(|v| &v.birth))
                    .push_bind(ext.map(|v| &v.region))
                    .push_bind(ext.map(|v| &v.gender))
                    .push_bind(ext.map(|v| &v.comment))
                    .push_bind(ext.map(|v| &v.twitter_account))
                    .push_bind(ext.map(|v| &v.web_page))
                    .push_bind(ext.map(|v| {
                        v.workspace.as_ref().map_or_else(
                            || "{}".to_string(),
                            |w| serde_json::to_string(&w).unwrap(),
                        )
                    }));
            });
            let query = query_builder.build();
            query.execute(&mut tr).await.unwrap();
        }
        tr.commit().await.unwrap();
    }
    #[tokio::test]
    async fn users() {
        users_().await
    }

    async fn tags_() {
        let mdb = get_mongo().await;
        let collection = mdb.collection::<Tag>("pixiv_tag");
        let old: Vec<_> = collection
            .find(None, None)
            .await
            .unwrap()
            .try_collect()
            .await
            .unwrap();

        let pool = get_mysql().await;
        let mut tr = pool.begin().await.unwrap();
        for tag in old {
            let mut q = QueryBuilder::<MySql>::new("INSERT INTO pixiv_tag (protected) ");
            q.push_values([&tag], |mut b, tag| {
                b.push_bind(tag.protected);
            });

            let query = q.build();
            let tag_id = query.execute(&pool).await.unwrap().last_insert_id();

            let mut q = QueryBuilder::<MySql>::new(
                "INSERT INTO pixiv_tag_alias (
                    tag_id,
                    alias
                ) ",
            );
            q.push_values(&tag.alias, |mut b, alias| {
                b.push_bind(tag_id).push_bind(alias);
            });
            q.build().execute(&mut tr).await.unwrap();
        }
        tr.commit().await.unwrap();
    }
    #[tokio::test]
    async fn tags() {
        tags_().await
    }

    async fn illust_() {
        let mdb = get_mongo().await;
        let collection = mdb.collection::<PixivIllust>("pixiv_illust");
        let c_user = mdb.collection::<PixivUser>("pixiv_user");
        let c_tag = mdb.collection::<Tag>("pixiv_tag");
        let old: Vec<_> = collection
            .find(None, None)
            .await
            .unwrap()
            .try_collect()
            .await
            .unwrap();

        let pool = get_mysql().await;
        let mut tr = pool.begin().await.unwrap();

        for illust in old {
            let parent = c_user
                .find_one(
                    doc! {
                        "_id": illust.parent_id
                    },
                    None,
                )
                .await
                .unwrap();
            let mut q = QueryBuilder::<MySql>::new(
                "SELECT @parent_id := id FROM pixiv_user WHERE source_id = ",
            );
            q.push_bind(parent.as_ref().map(|v| &v.source_id));
            q.build().execute(&mut tr).await.unwrap();

            let mut q = QueryBuilder::<MySql>::new(
                "INSERT INTO pixiv_illust (
                        parent_id,
                        source_id,
                        source_inaccessible,
                        last_modified,
                        total_bookmarks,
                        total_view,
                        is_bookmarked
                    ) ",
            );
            q.push_values([&illust], |mut b, illust| {
                b.push("@parent_id")
                    .push_bind(&illust.source_id)
                    .push_bind(illust.source_inaccessible)
                    .push_bind(illust.last_modified.map(|dt| dt.to_chrono()))
                    .push_bind(illust.extension.as_ref().map(|v| v.total_bookmarks))
                    .push_bind(illust.extension.as_ref().map(|v| v.total_view))
                    .push_bind(illust.extension.as_ref().map(|v| v.is_bookmarked));
            });
            q.build().execute(&mut tr).await.unwrap();

            sqlx::query("SET @item_id = LAST_INSERT_ID()")
                .execute(&mut tr)
                .await
                .unwrap();

            if !illust.tag_ids.is_empty() {
                let tags: Vec<_> = c_tag
                    .find(
                        doc! {
                            "_id": {
                                "$in": &illust.tag_ids
                            }
                        },
                        None,
                    )
                    .await
                    .unwrap()
                    .try_collect()
                    .await
                    .unwrap();
                let aliases: Vec<_> = tags.into_iter().flat_map(|tag| tag.alias).collect();
                if !aliases.is_empty() {
                    let mut q = QueryBuilder::<MySql>::new(
                        "
                    insert into pixiv_illust_tag
                    select @item_id as illust_id, pixiv_tag.id as tag_id
                    from pixiv_tag
                    inner join pixiv_tag_alias pta on pixiv_tag.id = pta.tag_id
                    where (pta.alias) in ",
                    );
                    q.push_tuples(aliases, |mut b, alias| {
                        b.push_bind(alias);
                    });
                    q.push(" group by tag_id");
                    q.build().execute(&mut tr).await.unwrap();
                }
            }

            for h in illust.history {
                let ext = h.extension.as_ref().unwrap();
                let mut q = QueryBuilder::<MySql>::new(
                    "INSERT INTO pixiv_illust_history (
                        item_id,
                        last_modified,
                        illust_type,
                        caption_html,
                        title,
                        date
                    ) ",
                );
                q.push_values([&h], |mut b, h| {
                    b.push("@item_id")
                        .push_bind(h.last_modified.map(|dt| dt.to_chrono()))
                        .push_bind(&ext.illust_type)
                        .push_bind(&ext.caption_html)
                        .push_bind(&ext.title)
                        .push_bind(
                            h.extension
                                .as_ref()
                                .map(|v| v.date.map(|dt| dt.to_chrono())),
                        );
                });
                q.build().execute(&mut tr).await.unwrap();

                if !ext.image_urls.is_empty() {
                    sqlx::query("SET @history_id = LAST_INSERT_ID()")
                        .execute(&mut tr)
                        .await
                        .unwrap();

                    let mut q = QueryBuilder::<MySql>::new(
                        "
                    insert into pixiv_illust_history_media
                    select @history_id as history_id, pixiv_media.id as media_id
                    from pixiv_media
                    where (pixiv_media.url) in ",
                    );
                    q.push_tuples(&ext.image_urls, |mut b, url| {
                        b.push_bind(url);
                    });
                    q.build().execute(&mut tr).await.unwrap();
                }
            }
        }

        tr.commit().await.unwrap();
    }
    #[tokio::test]
    async fn illust() {
        illust_().await
    }

    async fn novel_() {
        let mdb = get_mongo().await;
        let collection = mdb.collection::<PixivNovel>("pixiv_novel");
        let c_user = mdb.collection::<PixivUser>("pixiv_user");
        let c_tag = mdb.collection::<Tag>("pixiv_tag");
        let old: Vec<_> = collection
            .find(None, None)
            .await
            .unwrap()
            .try_collect()
            .await
            .unwrap();

        let pool = get_mysql().await;
        let mut tr = pool.begin().await.unwrap();

        for novel in old {
            let parent = c_user
                .find_one(
                    doc! {
                        "_id": novel.parent_id
                    },
                    None,
                )
                .await
                .unwrap();
            let mut q = QueryBuilder::<MySql>::new(
                "SELECT @parent_id := id FROM pixiv_user WHERE source_id = ",
            );
            q.push_bind(parent.as_ref().map(|v| &v.source_id));
            q.build().execute(&mut tr).await.unwrap();

            let mut q = QueryBuilder::<MySql>::new(
                "INSERT INTO pixiv_novel (
                        parent_id,
                        source_id,
                        source_inaccessible,
                        last_modified,
                        total_bookmarks,
                        total_view,
                        is_bookmarked
                    ) ",
            );
            q.push_values([&novel], |mut b, novel| {
                b.push("@parent_id")
                    .push_bind(&novel.source_id)
                    .push_bind(novel.source_inaccessible)
                    .push_bind(novel.last_modified.map(|dt| dt.to_chrono()))
                    .push_bind(novel.extension.as_ref().map(|v| v.total_bookmarks))
                    .push_bind(novel.extension.as_ref().map(|v| v.total_view))
                    .push_bind(novel.extension.as_ref().map(|v| v.is_bookmarked));
            });
            q.build().execute(&mut tr).await.unwrap();

            sqlx::query("SET @item_id = LAST_INSERT_ID()")
                .execute(&mut tr)
                .await
                .unwrap();

            if !novel.tag_ids.is_empty() {
                let tags: Vec<_> = c_tag
                    .find(
                        doc! {
                            "_id": {
                                "$in": &novel.tag_ids
                            }
                        },
                        None,
                    )
                    .await
                    .unwrap()
                    .try_collect()
                    .await
                    .unwrap();
                let aliases: Vec<_> = tags.into_iter().flat_map(|tag| tag.alias).collect();
                if !aliases.is_empty() {
                    let mut q = QueryBuilder::<MySql>::new(
                        "
                    insert into pixiv_novel_tag
                    select @item_id as novel_id, pixiv_tag.id as tag_id
                    from pixiv_tag
                    inner join pixiv_tag_alias pta on pixiv_tag.id = pta.tag_id
                    where (pta.alias) in ",
                    );
                    q.push_tuples(aliases, |mut b, alias| {
                        b.push_bind(alias);
                    })
                    .push(" group by tag_id");
                    q.build().execute(&mut tr).await.unwrap();
                }
            }

            for h in novel.history {
                let ext = h.extension.as_ref().unwrap();
                let mut q = QueryBuilder::<MySql>::new(
                    "INSERT INTO pixiv_novel_history (
                        item_id,
                        last_modified,
                        title,
                        caption_html,
                        text,
                        date
                    ) ",
                );
                q.push_values([&h], |mut b, h| {
                    b.push("@item_id")
                        .push_bind(h.last_modified.map(|dt| dt.to_chrono()))
                        .push_bind(&ext.title)
                        .push_bind(&ext.caption_html)
                        .push_bind(&ext.text)
                        .push_bind(
                            h.extension
                                .as_ref()
                                .map(|v| v.date.map(|dt| dt.to_chrono())),
                        );
                });
                q.build().execute(&mut tr).await.unwrap();
            }
        }
        tr.commit().await.unwrap();
    }
    #[tokio::test]
    async fn novel() {
        novel_().await
    }
}
