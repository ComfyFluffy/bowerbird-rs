use bowerbird_utils::{try_skip, Hsv};
use chrono::{Duration, Utc};
use log::{info, warn};
use path_slash::PathBufExt;
use snafu::ResultExt;
use sqlx::{query, PgPool, Postgres};
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    convert::TryInto,
    path::{Path, PathBuf},
};

use crate::{download::download_other_images, error, utils::parse_birth};

use super::PixivKit;
type QueryBuilder<'a> = sqlx::QueryBuilder<'a, Postgres>;
pub(crate) type Transaction = sqlx::Transaction<'static, Postgres>;

async fn update_users(
    users: impl Iterator<Item = &pixivcrab::models::user::User>,
    db: &PgPool,
    out_of_date_duration: Duration,
    mut on_need_update: impl FnMut(&str),
) -> crate::Result<()> {
    let mut tx = db.begin().await.context(error::Database)?;
    for user in users {
        let user_id = user.id.to_string();
        // insert or update user
        let updated_at = {
            query!(
                "
            insert into pixiv_user (source_id, source_inaccessible, is_followed) values ($1, $2, $3)
            on conflict (source_id) do update set source_inaccessible = $2, is_followed = $3
            returning updated_at
            ",
                &user_id,
                true,
                user.is_followed
            )
            .fetch_one(&mut tx)
            .await
            .context(error::Database)?
            .updated_at
        };

        let avatar_url = query!(
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
            &user_id
        )
        .fetch_optional(&mut tx)
        .await
        .context(error::Database)?
        .and_then(|row| row.url);

        let user_up_to_date = || {
            if Some(&user.profile_image_urls.medium) != avatar_url.as_ref() {
                return false;
            }

            if let Some(updated_at) = updated_at {
                Utc::now() - updated_at < out_of_date_duration
            } else {
                false
            }
        };
        // Do not insert to update list if updated within the given duration and the avatar is up-to-date.
        if !user_up_to_date() {
            on_need_update(&user_id);
        }
    }
    tx.commit().await.context(error::Database)?;
    Ok(())
}

async fn update_tags(
    tags: impl Iterator<Item = &pixivcrab::models::Tag>,
    db: &PgPool,
    // mut on_id_returned: Option<impl FnMut(&Vec<String>, i32)>,
) -> crate::Result<()> {
    let tags: Vec<Vec<String>> = flatten_tags_and_insert(tags)
        .into_iter()
        .map(|x| x.into_iter().map(|x| x.to_string()).collect())
        .collect();
    let mut tx = db.begin().await.context(error::Database)?;
    for alias in tags {
        let id = query!(
            "
            select id from pixiv_tag where alias && $1::varchar[]
            ",
            &alias
        )
        .fetch_optional(&mut tx)
        .await
        .context(error::Database)?
        .map(|row| row.id);

        if let Some(id) = id {
            query!(
                "
                update pixiv_tag set
                    alias = ARRAY(select distinct unnest(alias || $1::varchar[]))
                where id = $2
                ",
                &alias,
                id
            )
            .execute(&mut tx)
            .await
            .context(error::Database)?;
        } else {
            query!(
                "
                insert into pixiv_tag (alias) values ($1)
                ",
                &alias
            )
            .execute(&mut tx)
            .await
            .context(error::Database)?;
        };
    }
    tx.commit().await.context(error::Database)?;

    Ok(())
}

fn flatten_alias(tag: &pixivcrab::models::Tag) -> Vec<&str> {
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

fn flatten_tags_and_insert<'a>(
    tags: impl Iterator<Item = &'a pixivcrab::models::Tag>,
) -> HashSet<Vec<&'a str>> {
    tags.map(flatten_alias).filter(|v| !v.is_empty()).collect()
}

async fn set_source_inaccessible(
    tx: &mut Transaction,
    table_name: &str,
    source_id: &str,
) -> crate::Result<()> {
    warn!("pixiv: Works {} is invisible!", source_id);
    sqlx::query(&format!(
        "
        UPDATE {table_name}
        SET source_inaccessible = 1
        WHERE source_id = $1
        "
    ))
    .bind(source_id)
    .execute(tx)
    .await
    .context(error::Database)?;
    Ok(())
}

pub async fn update_user_id_set(
    users_need_update_set: BTreeSet<String>,
    kit: &PixivKit,
) -> crate::Result<()> {
    let need_sleep = users_need_update_set.len() > kit.config.pixiv.user_update_interval_threshold;
    // Sleep for 500ms to avoid 403 error
    for user_id in users_need_update_set {
        try_skip!(update_user_detail(&user_id, kit).await);
        if need_sleep {
            tokio::time::sleep(kit.config.pixiv.user_update_interval).await;
        }
    }
    Ok(())
}

async fn update_user_detail(user_id: &str, kit: &PixivKit) -> crate::Result<()> {
    info!("updating pixiv user data: {}", user_id);
    let resp = kit
        .api
        .user_detail(user_id)
        .await
        .context(error::PixivApi)?;

    let user = resp.user;
    let profile = resp.profile;
    let mut tx = kit.db.begin().await.context(error::Database)?;

    let id = query!(
        "
        insert into pixiv_user (source_id, source_inaccessible, updated_at, is_followed, total_following,
            total_illust_series, total_illusts, total_manga, total_novel_series, total_novels,
            total_public_bookmarks) values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
        on conflict (source_id) do update set
            source_inaccessible = $2,
            updated_at = $3,
            is_followed = $4,
            total_following = $5,
            total_illust_series = $6,
            total_illusts = $7,
            total_manga = $8,
            total_novel_series = $9,
            total_novels = $10,
            total_public_bookmarks = $11
        returning id
        ",
        user_id,
        false,
        Utc::now(),
        user.is_followed,
        profile.total_follow_users,
        profile.total_illust_series,
        profile.total_illusts,
        profile.total_manga,
        profile.total_novel_series,
        profile.total_novels,
        profile.total_illust_bookmarks_public
    ).fetch_one(&mut tx).await.context(error::Database)?.id;

    fn not_empty(s: &String) -> bool {
        !s.is_empty()
    }
    let mut workspace = resp.workspace;
    let workspace_image_url = workspace
        .remove("workspace_image_url")
        .unwrap_or_default()
        .filter(not_empty);
    let avatar_url = Some(user.profile_image_urls.medium).filter(not_empty);
    let background_url = profile.background_image_url.filter(not_empty);

    let workspace: BTreeMap<String, String> = workspace
        .into_iter()
        .filter_map(|(k, v)| v.filter(not_empty).map(|v| (k, v)))
        .collect();

    {
        let urls = [&workspace_image_url, &avatar_url, &background_url]
            .into_iter()
            .filter(|x| x.is_some());

        let mut q = QueryBuilder::new("insert into pixiv_media (url) ");
        q.push_values(urls, |mut b, v| {
            b.push_bind(v);
        });
        q.push("on conflict (url) do nothing");
        q.build().execute(&mut tx).await.context(error::Database)?;
    }

    if let Some(ref avatar_url) = avatar_url {
        download_other_images("avatar", avatar_url, kit).await?;
    }
    if let Some(ref background_url) = background_url {
        download_other_images("background", background_url, kit).await?;
    }
    if let Some(ref workspace_image_url) = workspace_image_url {
        download_other_images("workspace", workspace_image_url, kit).await?;
    }

    query!(
        "
        insert into pixiv_user_history (item_id, workspace_image_id, background_id, avatar_id, inserted_at, birth, region,
            gender, comment, twitter_account, web_page, workspace)
        select 
            $1,
            (select id from pixiv_media where url = $2),
            (select id from pixiv_media where url = $3),
            (select id from pixiv_media where url = $4),
            now(),
            $5,
            $6::varchar,
            $7::varchar,
            $8,
            $9::varchar,
            $10::varchar,
            $11
        where not exists (
            select id from pixiv_user_history where item_id = $1
            and workspace_image_id = (select id from pixiv_media where url = $2)
            and background_id = (select id from pixiv_media where url = $3)
            and avatar_id = (select id from pixiv_media where url = $4)
            and birth = $5
            and region = $6
            and gender = $7
            and comment = $8
            and twitter_account = $9
            and web_page = $10
            and workspace = $11
        )
        ",
        id,
        workspace_image_url,
        background_url,
        avatar_url,
        parse_birth(&profile.birth),
        profile.region,
        profile.gender,
        user.comment,
        profile.twitter_account,
        profile.webpage,
        serde_json::to_value(&workspace).unwrap_or_default()
    ).execute(&mut tx).await.context(error::Database)?;
    tx.commit().await.context(error::Database)?;
    Ok(())
}

pub async fn save_image(
    db: &PgPool,
    size: i32,
    (w, h): (i32, i32),
    palette_hsv: Vec<Hsv>,
    url: String,
    path: impl AsRef<Path>,
    path_db: String,
) -> crate::Result<()> {
    let mime = mime_guess::from_path(path).first().map(|x| x.to_string());
    let mut tx = db.begin().await.context(error::Database)?;
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
        path_db,
        w,
        h,
        url
    )
    .fetch_one(&mut tx)
    .await
    .context(error::Database)?
    .id;
    // Use insert into ... select ... where not exists ... to avoid duplicate.
    let mut q = QueryBuilder::new(
        "
        insert into pixiv_media_color (media_id, h, s, v)
        select * from (
            ",
    );
    q.push_values(palette_hsv, |mut b, v| {
        b.push_bind(id);
        b.push_bind(v[0]);
        b.push_bind(v[1]);
        b.push_bind(v[2]);
    });
    q.push(
        "
        ) as data
        where not exists (select id from pixiv_media_color where media_id = $1)
        ",
    );
    q.build().execute(&mut tx).await.context(error::Database)?;
    tx.commit().await.context(error::Database)?;
    Ok(())
}

pub async fn save_image_ugoira(
    db: &PgPool,
    zip_url: String,
    mut zip_path: PathBuf,
    zip_path_db: String,
    zip_size: i32,
    with_mp4: bool,
) -> anyhow::Result<()> {
    let mut tx = db.begin().await.context(error::Database)?;
    query!(
        "
        insert into pixiv_media (url, local_path, mime, size)
        values ($1, $2, $3, $4)
        on conflict (url) do nothing
        ",
        zip_url,
        zip_path_db,
        "application/zip",
        zip_size
    )
    .execute(&mut tx)
    .await
    .context(error::Database)?;

    if with_mp4 {
        let mut zip_path_db_slash = PathBuf::from_slash(zip_path_db);
        zip_path_db_slash.set_extension("mp4");
        let mp4_path_db = zip_path_db_slash.to_slash_lossy().to_string();

        zip_path.set_extension("mp4");
        let mp4_path = zip_path;

        let size: i32 = tokio::fs::metadata(&mp4_path)
            .await?
            .len()
            .try_into()
            .unwrap_or_default();

        query!(
            "
            insert into pixiv_media (local_path, mime, size)
            select $1::varchar, $2, $3
            where not exists (select id from pixiv_media where local_path = $1)
            ",
            mp4_path_db,
            "video/mp4",
            size
        )
        .execute(&mut tx)
        .await
        .context(error::Database)?;
    }
    tx.commit().await.context(error::Database)?;
    Ok(())
}

pub async fn save_illusts(
    illusts: &Vec<pixivcrab::models::illust::Illust>,
    kit: &PixivKit,
    on_user_need_update: impl FnMut(&str),
    mut on_ugoira_metadata: impl FnMut(&str, (&str, &[i32])),
) -> crate::Result<()> {
    update_tags(illusts.iter().flat_map(|x| &x.tags), &kit.db).await?;
    update_users(
        illusts
            .iter()
            .filter_map(|x| Some(&x.user).filter(|u| u.id != 0)),
        &kit.db,
        Duration::days(7), // TODO: use env var
        on_user_need_update,
    )
    .await?;

    for i in illusts {
        let id = i.id.to_string();
        let delay = if i.r#type == "ugoira" {
            let ugoira = kit
                .api
                .ugoira_metadata(&id)
                .await
                .context(error::PixivApi)?;
            let delay: Vec<i32> = ugoira
                .ugoira_metadata
                .frames
                .iter()
                .map(|frame| frame.delay)
                .collect();
            on_ugoira_metadata(&id, (&ugoira.ugoira_metadata.zip_urls.medium, &delay));
            Some(delay)
        } else {
            None
        };
        let delay_slice = delay.as_deref();

        let mut tx = kit.db.begin().await.context(error::Database)?;
        if !i.visible {
            if i.id != 0 {
                set_source_inaccessible(&mut tx, "pixiv_illust", &id).await?;
            }
            continue;
        }

        let alias: Vec<String> = flatten_tags_alias(i.tags.iter())
            .into_iter()
            .map(|x| x.to_string())
            .collect();
        let item_id = query!(
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
            i.user.id.to_string(),
            &id,
            i.total_bookmarks,
            i.total_view,
            i.is_bookmarked,
            &alias
        )
        .fetch_one(&mut tx)
        .await
        .context(error::Database)?
        .id;

        let urls: Vec<String> = if i.page_count <= 1 {
            i.meta_single_page
                .original_image_url
                .clone()
                .into_iter()
                .collect()
        } else {
            i.meta_pages
                .iter()
                .filter_map(|x| x.image_urls.original.clone())
                .collect()
        };

        let mut q = QueryBuilder::new("insert into pixiv_media (url) ");
        q.push_values(&urls, |mut b, v| {
            b.push_bind(v);
        });
        q.push(" on conflict (url) do nothing");
        q.build().execute(&mut tx).await.context(error::Database)?;

        query!(
            "
            insert into pixiv_illust_history (item_id, illust_type, caption_html, title, date, media_ids, ugoira_frame_duration)
            select
                $1,
                $2::varchar,
                $3,
                $4::varchar,
                $5,
                (select array_agg(id order by i)
                from (SELECT id, urls.i i
                      FROM pixiv_media
                               join unnest(
                                $6::varchar[]
                          )
                          with ordinality urls(url, i) using (url)) t),
                $7
            where not exists (select id from pixiv_illust_history where 
                item_id = $1 and illust_type = $2 and caption_html = $3 and title = $4 and date = $5
                and media_ids = (SELECT array_agg(id) FROM pixiv_media join unnest(
                    $6::varchar[]
                ) on unnest = pixiv_media.url)
                and ugoira_frame_duration = $7
            )
            ",
            item_id,
            i.r#type,
            i.caption,
            i.title,
            i.create_date,
            &urls,
            delay_slice
        ).execute(&mut tx).await.context(error::Database)?;
        tx.commit().await.context(error::Database)?;
    }
    Ok(())
}

pub async fn save_novels(
    novels: &Vec<pixivcrab::models::novel::Novel>,
    update_exists: bool,
    kit: &PixivKit,
    mut on_each_should_continue: impl FnMut() -> bool,
    on_user_need_update: impl FnMut(&str),
) -> crate::Result<()> {
    update_tags(novels.iter().flat_map(|x| &x.tags), &kit.db).await?;
    update_users(
        novels
            .iter()
            .filter_map(|x| Some(&x.user).filter(|u| u.id != 0)),
        &kit.db,
        Duration::days(7), // TODO: use env var
        on_user_need_update,
    )
    .await?;

    for n in novels {
        let id = n.id.to_string();
        info!("pixiv: getting novel text of {}", id);
        let r = kit.api.novel_text(&id).await.context(error::PixivApi)?;

        let mut tx = kit.db.begin().await.context(error::Database)?;
        if !on_each_should_continue() {
            return Ok(());
        }
        if !n.visible {
            if n.id != 0 {
                set_source_inaccessible(&mut tx, "pixiv_novel", &id).await?;
            }
            continue;
        }

        let alias: Vec<String> = flatten_tags_alias(n.tags.iter())
            .into_iter()
            .map(|x| x.to_string())
            .collect();
        let item_id = query!(
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
            &id,
            n.total_bookmarks,
            n.total_view,
            n.is_bookmarked,
            &alias
        )
        .fetch_one(&mut tx)
        .await
        .context(error::Database)?
        .id;

        let history_exists = query!(
            "
            select id from pixiv_novel_history where item_id = $1 limit 1
            ",
            item_id
        )
        .fetch_optional(&mut tx)
        .await
        .context(error::Database)?
        .is_some();
        if history_exists && !update_exists {
            continue;
        }

        query!(
            "
            insert into pixiv_novel_history (item_id, title, date, caption_html, text)
            select
                $1,
                $2::varchar,
                $3,
                $4,
                $5
            where not exists (select id from pixiv_novel_history where 
                item_id = $1 and title = $2 and date = $3 and caption_html = $4 and text = $5
            )
            ",
            item_id,
            n.title,
            n.create_date.naive_utc(),
            n.caption,
            r.novel_text
        )
        .execute(&mut tx)
        .await
        .context(error::Database)?;
        tx.commit().await.context(error::Database)?;
    }

    Ok(())
}
