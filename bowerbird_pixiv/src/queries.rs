use std::collections::HashSet;

use chrono::{DateTime, Utc};
use snafu::ResultExt;
use sqlx::{query, query_unchecked, PgExecutor, QueryBuilder};

use crate::{error, Result};

/// Equal, treating null as a comparable value.
///
/// Used for upserting.
///
/// https://www.postgresql.org/docs/current/functions-comparison.html
const EQ: &str = "IS NOT DISTINCT FROM";

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
) -> crate::Result<()> {
    query(&format!(
        "
        UPDATE {table_name}
        SET source_inaccessible = 1
        WHERE source_id = $1
        "
    ))
    .bind(source_id)
    .execute(e)
    .await
    .context(error::Database)?;
    Ok(())
}

pub mod user {
    use std::collections::BTreeMap;

    use pixivcrab::models::user::{Profile, User, Workspace};

    use crate::utils::parse_birth;

    use super::*;

    pub async fn avatar_url_by_user_id(
        user_id: &str,
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
            user_id
        )
        .fetch_optional(e)
        .await
        .context(error::Database)?
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
                insert into pixiv_user (source_id, source_inaccessible, is_followed) values ($1, $2, $3)
                on conflict (source_id) do update set source_inaccessible = $2, is_followed = $3
                returning updated_at
                ",
                &user_id,
                true,
                is_followed
            )
            .fetch_one(e)
            .await
            .context(error::Database)?
            .updated_at
        };
        Ok(updated_at)
    }

    pub async fn update_item_returning_id(
        source_id: &str,
        user: &User,
        profile: &Profile,
        e: impl PgExecutor<'_>,
    ) -> Result<i32> {
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
        .context(error::Database)?
        .id;
        Ok(id)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_history(
        item_id: i32,
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
                $x.as_ref().map(|x| x.as_str()).filter(|x| !x.is_empty())
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

        let workspace_image_id = "(select id from pixiv_media where url = $2)";
        let background_id = "(select id from pixiv_media where url = $3)";
        let avatar_id = "(select id from pixiv_media where url = $4)";
        query(
        &format!("
            insert into pixiv_user_history (item_id, workspace_image_id, background_id, avatar_id, inserted_at, account,
                name, is_premium, birth, region, gender, comment, twitter_account, web_page,
                workspace)
            select 
                $1,
                {workspace_image_id},
                {background_id},
                {avatar_id},
                now(),
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
            where not exists (
                select id from pixiv_user_history where
                item_id = $1
                and workspace_image_id {EQ} {workspace_image_id}
                and background_id {EQ} {background_id}
                and avatar_id {EQ} {avatar_id}
                and account {EQ} $5
                and name {EQ} $6
                and is_premium {EQ} $7
                and birth {EQ} $8
                and region {EQ} $9
                and gender {EQ} $10
                and comment {EQ} $11
                and twitter_account {EQ} $12
                and web_page {EQ} $13
                and workspace {EQ} $14
            )
            "),
        )
        .bind(item_id)
        .bind(&workspace_image_url)
        .bind(&background_url)
        .bind(&avatar_url)
        .bind(&user.account)
        .bind(&user.name)
        .bind(profile.is_premium)
        .bind(parse_birth(&profile.birth))
        .bind(region)
        .bind(gender)
        .bind(comment)
        .bind(twitter_account)
        .bind(web_page)
        .bind(workspace)
        .execute(e)
        .await
        .context(error::Database)?;
        Ok(())
    }
}

pub mod tag {
    use super::*;

    pub async fn id_by_alias_match(
        alias: &[String],
        e: impl PgExecutor<'_>,
    ) -> Result<Option<i32>> {
        let id = query!(
            "
            select id from pixiv_tag where alias && $1::varchar[]
            ",
            &alias
        )
        .fetch_optional(e)
        .await
        .context(error::Database)?
        .map(|row| row.id);
        Ok(id)
    }

    pub async fn update_unique(id: i32, alias: &[String], e: impl PgExecutor<'_>) -> Result<()> {
        query!(
            "
            update pixiv_tag set
                alias = ARRAY(select distinct unnest(alias || $1::varchar[]))
            where id = $2
            ",
            &alias,
            id
        )
        .execute(e)
        .await
        .context(error::Database)?;
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
        .context(error::Database)?;
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
    ) -> Result<i32> {
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
        .context(error::Database)?
        .id;
        Ok(id)
    }

    pub async fn insert_colors(media_id: i32, hsv: &[Hsv], e: impl PgExecutor<'_>) -> Result<()> {
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
        q.build().execute(e).await.context(error::Database)?;
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
        .context(error::Database)?;
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
        .context(error::Database)?;
        Ok(())
    }
}

pub mod illust {
    use pixivcrab::models::illust::Illust;

    use super::*;

    pub async fn upsert_item_returning_id(illust: &Illust, e: impl PgExecutor<'_>) -> Result<i32> {
        let alias: Vec<String> = flatten_tags_alias(illust.tags.iter())
            .into_iter()
            .map(|x| x.to_string())
            .collect();
        let id = query!(
            "
            insert into pixiv_illust (parent_id, source_id, total_bookmarks, total_view,
                is_bookmarked, tag_ids, source_inaccessible, updated_at)
            values (
                (select id from pixiv_user where source_id = $1),
                $2,
                $3,
                $4,
                $5, 
                (select array_agg(id) from pixiv_tag where alias && $6::varchar[]),
                false,
                now()
            )
            on conflict (source_id) do update set
                total_bookmarks = $3,
                total_view = $4,
                is_bookmarked = $5,
                tag_ids = (select array_agg(id) from pixiv_tag where alias && $6::varchar[]),
                source_inaccessible = false,
                updated_at = now()
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
        .context(error::Database)?
        .id;
        Ok(id)
    }

    pub async fn insert_history(
        item_id: i32,
        illust: &Illust,
        image_urls: &[&str],
        delay_slice: Option<&[i32]>,
        e: impl PgExecutor<'_>,
    ) -> Result<()> {
        let media_ids = "
        (select array_agg(id order by i)
        from pixiv_media
               join unnest(
                   $6::varchar[]
               )
               with ordinality urls(url, i) using (url))
        ";

        query(
            &format!("
            insert into pixiv_illust_history (item_id, illust_type, caption_html, title, date, media_ids, ugoira_frame_duration)
            select
                $1,
                $2::varchar,
                $3,
                $4::varchar,
                $5,
                {media_ids},
                $7
            where not exists (
                select id from pixiv_illust_history where 
                item_id = $1
                and illust_type {EQ} $2
                and caption_html {EQ} $3
                and title {EQ} $4
                and date {EQ} $5
                and media_ids {EQ} {media_ids}
                and ugoira_frame_duration {EQ} $7
            )
            ")
        )
        .bind(item_id)
        .bind(&illust.r#type)
        .bind(&illust.caption)
        .bind(&illust.title)
        .bind(illust.create_date)
        .bind(image_urls)
        .bind(delay_slice)
        .execute(e)
        .await
        .context(error::Database)?;
        Ok(())
    }
}

pub mod novel {
    use pixivcrab::models::novel::Novel;

    use super::*;

    pub async fn upsert_item_returning_id(n: &Novel, e: impl PgExecutor<'_>) -> Result<i32> {
        let alias: Vec<&str> = flatten_tags_alias(n.tags.iter()).into_iter().collect();
        let id = query_unchecked!(
            "
            insert into pixiv_novel (parent_id, source_id, total_bookmarks, total_view,
                is_bookmarked, tag_ids, source_inaccessible, updated_at)
            values (
                (select id from pixiv_user where source_id = $1),
                $2,
                $3,
                $4,
                $5,
                (select array_agg(id) from pixiv_tag where alias && $6::varchar[]),
                false,
                now()
            )
            on conflict (source_id) do update set
                total_bookmarks = $3,
                total_view = $4,
                is_bookmarked = $5,
                tag_ids = (select array_agg(id) from pixiv_tag where alias && $6::varchar[]),
                source_inaccessible = false,
                updated_at = now()
            returning id
            ",
            n.user.id.to_string(),
            &n.id.to_string(),
            n.total_bookmarks,
            n.total_view,
            n.is_bookmarked,
            alias
        )
        .fetch_one(e)
        .await
        .context(error::Database)?
        .id;
        Ok(id)
    }

    pub async fn history_exists(item_id: i32, e: impl PgExecutor<'_>) -> Result<bool> {
        let exists = query!(
            "
            select id from pixiv_novel_history where item_id = $1 limit 1
            ",
            item_id
        )
        .fetch_optional(e)
        .await
        .context(error::Database)?
        .is_some();
        Ok(exists)
    }

    pub async fn insert_history(
        item_id: i32,
        n: &Novel,
        novel_text: &str,
        e: impl PgExecutor<'_>,
    ) -> Result<()> {
        query(&format!(
            "
            insert into pixiv_novel_history (item_id, title, date, caption_html, text)
            select
                $1,
                $2::varchar,
                $3,
                $4,
                $5
            where not exists (
                select id from pixiv_novel_history where 
                item_id = $1
                and title {EQ} $2
                and date {EQ} $3
                and caption_html {EQ} $4
                and text {EQ} $5
            )
            "
        ))
        .bind(item_id)
        .bind(&n.title)
        .bind(n.create_date.naive_utc())
        .bind(&n.caption)
        .bind(novel_text)
        .execute(e)
        .await
        .context(error::Database)?;

        Ok(())
    }
}
