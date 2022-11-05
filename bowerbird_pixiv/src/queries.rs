use std::collections::HashSet;

use chrono::{DateTime, Utc};
use snafu::ResultExt;
use sqlx::{query, query_unchecked, PgExecutor, QueryBuilder};

use crate::{error, Result};

pub fn flatten_alias(tag: &pixivcrab::models::Tag) -> Vec<&str> {
    let mut alias: Vec<&str> = vec![];
    if !tag.name.is_empty() {
        alias.push(&tag.name);
    }
    if let Some(ref translated_name) = tag.translated_name {
        if !translated_name.is_empty() {
            alias.push(translated_name);
        }
    }
    alias
}

fn flatten_tags_alias<'a>(
    tags: impl Iterator<Item = &'a pixivcrab::models::Tag>,
) -> HashSet<&'a str> {
    tags.flat_map(|tag| flatten_alias(tag).into_iter())
        .collect()
}

pub fn flatten_tags_and_insert<'a>(
    tags: impl Iterator<Item = &'a pixivcrab::models::Tag>,
) -> HashSet<Vec<&'a str>> {
    tags.map(flatten_alias).filter(|v| !v.is_empty()).collect()
}

pub async fn set_source_inaccessible(
    table_name: &str,
    source_id: &str,
    e: impl PgExecutor<'_>,
) -> Result<()> {
    query(&format!(
        "
        UPDATE {table_name}
        SET source_inaccessible = true
        WHERE source_id = $1
        "
    ))
    .bind(source_id)
    .execute(e)
    .await
    .with_context(|_| error::Database {
        message: format!("set_source_inaccessible: {:?}, {:?}", table_name, source_id),
    })?;
    Ok(())
}

pub mod user {
    use std::collections::BTreeMap;

    use pixivcrab::models::user::{Profile, User, Workspace};

    use crate::utils::parse_birth;

    use super::*;

    pub async fn avatar_url_by_source_id(
        source_id: &str,
        e: impl PgExecutor<'_>,
    ) -> Result<Option<String>> {
        Ok(query!(
            "
            select url
            from pixiv_media
            where id = (select avatar_id
                        from pixiv_user_history
                        where item_id = (select id
                                        from pixiv_user
                                        where source_id = $1)
                        order by id desc
                        limit 1)
            ",
            source_id
        )
        .fetch_optional(e)
        .await
        .with_context(|_| error::Database {
            message: format!("avatar_url_by_user_id: {:?}", source_id),
        })?
        .and_then(|row| row.url))
    }

    pub async fn upsert_basic_returning_updated_at(
        user_id: &str,
        is_followed: Option<bool>,
        e: impl PgExecutor<'_>,
    ) -> Result<Option<DateTime<Utc>>> {
        let updated_at = {
            query!(
                "
                insert into pixiv_user (source_id, source_inaccessible, is_followed) values ($1, true, $2)
                on conflict (source_id) do update set source_inaccessible = true, is_followed = $2
                returning updated_at
                ",
                &user_id,
                is_followed
            )
            .fetch_one(e)
            .await
            .with_context(|_| error::Database {
                message: format!("upsert_basic_returning_updated_at: {:?}", user_id)
            })?
            .updated_at
        };
        Ok(updated_at)
    }

    pub async fn update_item_returning_id(
        source_id: &str,
        user: &User,
        profile: &Profile,
        e: impl PgExecutor<'_>,
    ) -> Result<i64> {
        let id = query!(
            "
            update pixiv_user set
                updated_at = now(),
                source_inaccessible = false,
                is_followed = $1,
                total_following = $2,
                total_illust_series = $3,
                total_illusts = $4,
                total_manga = $5,
                total_novel_series = $6,
                total_novels = $7,
                total_public_bookmarks = $8
            where source_id = $9
            returning id
            ",
            user.is_followed,
            profile.total_follow_users,
            profile.total_illust_series,
            profile.total_illusts,
            profile.total_manga,
            profile.total_novel_series,
            profile.total_novels,
            profile.total_illust_bookmarks_public,
            source_id
        )
        .fetch_one(e)
        .await
        .with_context(|_| error::Database {
            message: format!(
                "update_item_returning_id: {:?}, {:?}, {:?}",
                source_id, user, profile
            ),
        })?
        .id;
        Ok(id)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_history(
        item_id: i64,
        user: &User,
        profile: &Profile,
        workspace: Workspace,

        workspace_image_url: Option<&str>,
        avatar_url: Option<&str>,
        background_url: Option<&str>,

        e: impl PgExecutor<'_>,
    ) -> Result<()> {
        macro_rules! filter_empty {
            ($x:expr) => {
                $x.as_deref().filter(|x| !x.is_empty())
            };
        }

        // Remove empty fields of workspace
        let workspace = Some(
            workspace
                .into_iter()
                .filter_map(|(k, v)| v.filter(|x| !x.is_empty()).map(|v| (k, v)))
                .collect::<BTreeMap<String, String>>(),
        )
        .and_then(|m| serde_json::to_value(m).ok());

        let comment = filter_empty!(user.comment);
        let gender = Some(profile.gender.as_str()).filter(|x| !x.is_empty());
        let twitter_account = filter_empty!(profile.twitter_account);
        let web_page = filter_empty!(profile.webpage);
        let region = filter_empty!(profile.region);

        query!(
            "
            insert into pixiv_user_history (item_id, workspace_image_id, background_id, avatar_id, account,
                                            name, is_premium, birth, region, gender, comment, twitter_account, web_page,
                                            workspace)
            select $1,
                   (select id from pixiv_media where url = $2),
                   (select id from pixiv_media where url = $3),
                   (select id from pixiv_media where url = $4),
                   $5::varchar,
                   $6::varchar,
                   $7,
                   $8,
                   $9::varchar,
                   $10::varchar,
                   $11,
                   $12::varchar,
                   $13::varchar,
                   $14
            where not exists(
                    select id
                    from pixiv_user_detail_latest_view
                    where id = $1
                      and workspace_image_url IS NOT DISTINCT FROM $2
                      and background_url IS NOT DISTINCT FROM $3
                      and avatar_url IS NOT DISTINCT FROM $4
                      and account IS NOT DISTINCT FROM $5
                      and name IS NOT DISTINCT FROM $6
                      and is_premium IS NOT DISTINCT FROM $7
                      and birth IS NOT DISTINCT FROM $8
                      and region IS NOT DISTINCT FROM $9
                      and gender IS NOT DISTINCT FROM $10
                      and comment IS NOT DISTINCT FROM $11
                      and twitter_account IS NOT DISTINCT FROM $12
                      and web_page IS NOT DISTINCT FROM $13
                      and workspace IS NOT DISTINCT FROM $14
                )
            ",
            item_id,
            workspace_image_url,
            background_url,
            avatar_url,
            &user.account,
            &user.name,
            profile.is_premium,
            parse_birth(&profile.birth),
            region,
            gender,
            comment,
            twitter_account,
            web_page,
            workspace,
        )
        .execute(e)
        .await
        .with_context(|_| error::Database {
            message: format!("update_history: {:?}, {:?}, {:?}", item_id, user, profile),
        })?;
        Ok(())
    }
}

pub mod tag {
    use super::*;

    pub async fn upsert_tag(alias: &[String], e: impl PgExecutor<'_>) -> Result<()> {
        query!(
            "
            select upsert_pixiv_tag($1)
            ",
            alias
        )
        .execute(e)
        .await
        .with_context(|_| error::Database {
            message: format!("upsert_tag: {:?}", alias),
        })?;
        Ok(())
    }
}

pub mod media {
    use bowerbird_utils::Hsv;

    use super::*;

    pub async fn insert_urls(urls: &[&str], e: impl PgExecutor<'_>) -> Result<()> {
        query_unchecked!(
            "
            insert into pixiv_media (url)
            select unnest($1::varchar[])
            on conflict (url) do nothing
            ",
            urls
        )
        .execute(e)
        .await
        .with_context(|_| error::Database {
            message: format!("insert_urls: {:?}", urls),
        })?;
        Ok(())
    }

    pub async fn update_returning_id(
        url: &str,
        size: i32,
        mime: Option<&str>,
        local_path: &str,
        width: Option<i32>,
        height: Option<i32>,
        e: impl PgExecutor<'_>,
    ) -> Result<i64> {
        let id = query!(
            "
            update pixiv_media set
                size = $1,
                mime = $2,
                local_path = $3,
                width = $4,
                height = $5
            where url = $6
            returning id
            ",
            size,
            mime,
            local_path,
            width,
            height,
            url
        )
        .fetch_one(e)
        .await
        .with_context(|_| error::Database {
            message: format!(
                "update_returning_id: {:?}, {:?}, {:?}, {:?}, {:?}, {:?}",
                url, size, mime, local_path, width, height
            ),
        })?
        .id;
        Ok(id)
    }

    pub async fn insert_colors(media_id: i64, hsv: &[Hsv], e: impl PgExecutor<'_>) -> Result<()> {
        let mut q = QueryBuilder::new(
            "
            insert into pixiv_media_color (media_id, h, s, v)
            select * from (
            ",
        );
        q.push_values(hsv, |mut b, hsv| {
            b.push_bind(media_id);
            for v in hsv {
                b.push_bind(v);
            }
        });
        q.push(
            "
            ) as data
            where not exists (select id from pixiv_media_color where media_id = $1)
            ",
        );
        q.build()
            .execute(e)
            .await
            .with_context(|_| error::Database {
                message: format!("insert_colors: {:?}, {:?}", media_id, hsv),
            })?;
        Ok(())
    }

    pub async fn insert_ugoira(
        url: &str,
        local_path: &str,
        size: i32,
        e: impl PgExecutor<'_>,
    ) -> Result<()> {
        query!(
            "
            insert into pixiv_media (url, local_path, mime, size)
            values ($1, $2, $3, $4)
            on conflict (url) do nothing
            ",
            url,
            local_path,
            "application/zip",
            size
        )
        .execute(e)
        .await
        .with_context(|_| error::Database {
            message: format!("insert_ugoira: {:?}, {:?}, {:?}", url, local_path, size),
        })?;
        Ok(())
    }

    pub async fn insert_ugoira_mp4(
        local_path: &str,
        size: i32,
        e: impl PgExecutor<'_>,
    ) -> Result<()> {
        query!(
            "
            insert into pixiv_media (local_path, mime, size)
            select $1::varchar, $2, $3
            where not exists (select id from pixiv_media where local_path = $1)
            ",
            local_path,
            "video/mp4",
            size
        )
        .execute(e)
        .await
        .with_context(|_| error::Database {
            message: format!("insert_ugoira_mp4: {:?}, {:?}", local_path, size),
        })?;
        Ok(())
    }

    pub async fn local_path_exists(local_path: &str, e: impl PgExecutor<'_>) -> Result<bool> {
        let r = query!(
            "
            select id from pixiv_media where local_path = $1 limit 1
            ",
            local_path
        )
        .fetch_optional(e)
        .await
        .with_context(|_| error::Database {
            message: format!("local_path_exists: {:?}", local_path),
        })?;
        Ok(r.is_some())
    }
}

pub mod illust {
    use pixivcrab::models::illust::Illust;

    use super::*;

    pub async fn upsert_item_returning_id(illust: &Illust, e: impl PgExecutor<'_>) -> Result<i64> {
        let alias: Vec<String> = flatten_tags_alias(illust.tags.iter())
            .into_iter()
            .map(|x| x.to_string())
            .collect();
        let id = query!(
            "
            insert into pixiv_illust (parent_id, source_id, total_bookmarks, total_view,
                                      is_bookmarked, tag_ids, source_inaccessible, updated_at)
            values ((select id from pixiv_user where source_id = $1),
                    $2,
                    $3,
                    $4,
                    $5,
                    (select array_agg(id) from pixiv_tag where alias && $6::varchar[]),
                    false,
                    now())
            on conflict (source_id) do update set total_bookmarks     = $3,
                                                  total_view          = $4,
                                                  is_bookmarked       = $5,
                                                  tag_ids             = (select array_agg(id) from pixiv_tag where alias && $6::varchar[]),
                                                  source_inaccessible = false,
                                                  updated_at          = now()
            returning id
            ",
            illust.user.id.to_string(),
            illust.id.to_string(),
            illust.total_bookmarks,
            illust.total_view,
            illust.is_bookmarked,
            &alias
        )
        .fetch_one(e)
        .await
        .with_context(|_| error::Database {
            message: format!("upsert_item_returning_id: {:?}", illust),
        })?
        .id;
        Ok(id)
    }

    pub async fn insert_history_returning_id(
        item_id: i64,
        illust: &Illust,
        urls: &[String],
        delay_slice: Option<&[i32]>,
        e: impl PgExecutor<'_>,
    ) -> Result<Option<i64>> {
        let id = query!(
            "
            insert into pixiv_illust_history (item_id, type_id, caption_html, title, date, ugoira_frame_duration)
            select $1,
                (select id from pixiv_illust_history_type where name = $2::varchar),
                $3,
                $4::varchar,
                $5,
                $6
            where not exists(
                    select id
                    from pixiv_illust_detail_lateral_view
                    where id = $1
                    and illust_type IS NOT DISTINCT FROM $2
                    and caption_html IS NOT DISTINCT FROM $3
                    and title IS NOT DISTINCT FROM $4
                    and date IS NOT DISTINCT FROM $5
                    and ugoira_frame_duration IS NOT DISTINCT FROM $6
                    and image_urls IS NOT DISTINCT FROM $7::varchar[]
                )
            returning id
            ",
            item_id,
            illust.r#type,
            illust.caption,
            illust.title,
            illust.create_date,
            delay_slice,
            urls
        )
        .fetch_optional(e)
        .await
        .with_context(|_| error::Database {
            message: format!("insert_history_returning_id: {:?}, {:?}", item_id, illust),
        })?
        .map(|r| r.id);
        Ok(id)
    }

    pub async fn insert_history_media(
        history_id: i64,
        media_urls: &[String],
        e: impl PgExecutor<'_>,
    ) -> Result<()> {
        query!(
            "
            insert into pixiv_illust_history_media (history_id, media_id)
            select $1, id
            from pixiv_media
                join unnest($2::varchar[]) url using (url)
            ",
            history_id,
            media_urls
        )
        .execute(e)
        .await
        .with_context(|_| error::Database {
            message: format!("insert_history_media: {:?}, {:?}", history_id, media_urls),
        })?;
        Ok(())
    }
}

pub mod novel {
    use pixivcrab::models::novel::Novel;

    use super::*;

    pub async fn upsert_item_returning_id(n: &Novel, e: impl PgExecutor<'_>) -> Result<i64> {
        let alias: Vec<&str> = flatten_tags_alias(n.tags.iter()).into_iter().collect();
        let id = query_unchecked!(
            "
            insert into pixiv_novel (parent_id, source_id, total_bookmarks, total_view,
                                     is_bookmarked, tag_ids, source_inaccessible, updated_at)
            values ((select id from pixiv_user where source_id = $1),
                    $2,
                    $3,
                    $4,
                    $5,
                    (select array_agg(id) from pixiv_tag where alias && $6::varchar[]),
                    false,
                    now())
            on conflict (source_id) do update set total_bookmarks     = $3,
                                                  total_view          = $4,
                                                  is_bookmarked       = $5,
                                                  tag_ids             = (select array_agg(id) from pixiv_tag where alias && $6::varchar[]),
                                                  source_inaccessible = false,
                                                  updated_at          = now()
            returning id
            ",
            n.user.id.to_string(),
            n.id.to_string(),
            n.total_bookmarks,
            n.total_view,
            n.is_bookmarked,
            alias
        )
        .fetch_one(e)
        .await
        .with_context(|_| error::Database {
            message: format!("upsert_item_returning_id: {:?}", n),
        })?
        .id;
        Ok(id)
    }

    pub async fn history_exists(item_id: i64, e: impl PgExecutor<'_>) -> Result<bool> {
        let exists = query!(
            "
            select id from pixiv_novel_history where item_id = $1 limit 1
            ",
            item_id
        )
        .fetch_optional(e)
        .await
        .with_context(|_| error::Database {
            message: format!("history_exists: {:?}", item_id),
        })?
        .is_some();
        Ok(exists)
    }

    pub async fn insert_history(
        item_id: i64,
        n: &Novel,
        novel_text: &str,
        e: impl PgExecutor<'_>,
    ) -> Result<()> {
        query!(
            "
            insert into pixiv_novel_history (item_id, title, date, caption_html, text)
            select $1,
                   $2::varchar,
                   $3,
                   $4,
                   $5
            where not exists(
                    select id
                    from pixiv_novel_detail_latest_view
                    where id = $1
                      and title IS NOT DISTINCT FROM $2
                      and date IS NOT DISTINCT FROM $3
                      and caption_html IS NOT DISTINCT FROM $4
                      and text IS NOT DISTINCT FROM $5
                )
            ",
            item_id,
            n.title,
            n.create_date,
            n.caption,
            novel_text,
        )
        .execute(e)
        .await
        .with_context(|_| error::Database {
            message: format!("insert_history: {:?}, {:?}", item_id, n),
        })?;

        Ok(())
    }
}
