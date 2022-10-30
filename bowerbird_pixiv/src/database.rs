use bowerbird_utils::{try_skip, ImageMetadata};
use chrono::Utc;
use log::{info, warn};
use path_slash::PathBufExt;
use snafu::ResultExt;
use sqlx::{query, PgPool};
use std::{
    collections::{BTreeSet, HashSet},
    convert::TryInto,
    path::{Path, PathBuf},
    time::Duration,
};

use crate::{download::download_other_image, error};
use crate::{queries::*, Result};

use super::PixivKit;

macro_rules! preprocess_items {
    ($items:expr, $db:expr, $out_of_date_duration:expr, $on_user_need_update:expr) => {
        update_tags($items.iter().flat_map(|x| &x.tags), $db).await?;
        update_users(
            $items
                .iter()
                .filter_map(|x| Some(&x.user).filter(|u| u.id != 0))
                .collect::<HashSet<_>>()
                .into_iter(),
            $db,
            $out_of_date_duration,
            $on_user_need_update,
        )
        .await?;
    };
}

async fn update_users(
    users: impl Iterator<Item = &pixivcrab::models::user::User>,
    db: &PgPool,
    out_of_date_duration: Duration,
    mut on_need_update: impl FnMut(&str),
) -> Result<()> {
    let mut tx = db.begin().await.context(error::DatabaseTransaction)?;
    #[allow(clippy::or_fun_call)] // The call is actually inlined as a constant
    let out_of_date_duration =
        chrono::Duration::from_std(out_of_date_duration).unwrap_or(chrono::Duration::max_value());
    for user in users {
        let user_id = user.id.to_string();

        let updated_at =
            user::upsert_basic_returning_updated_at(&user_id, user.is_followed, &mut tx).await?;

        let avatar_url = user::avatar_url_by_user_id(&user_id, &mut tx).await?;

        let user_up_to_date = (|| {
            if Some(&user.profile_image_urls.medium) != avatar_url.as_ref() {
                return false;
            }

            if let Some(updated_at) = updated_at {
                Utc::now() - updated_at < out_of_date_duration
            } else {
                false
            }
        })();
        // Do not insert to update list if updated within the given duration and the avatar is up-to-date.
        if !user_up_to_date {
            on_need_update(&user_id);
        }
    }
    tx.commit().await.context(error::DatabaseTransaction)?;
    Ok(())
}

async fn update_tags(
    tags: impl Iterator<Item = &pixivcrab::models::Tag>,
    db: &PgPool,
    // mut on_id_returned: Option<impl FnMut(&Vec<String>, i32)>,
) -> Result<()> {
    let tags: HashSet<Vec<String>> = flatten_tags_and_insert(tags)
        .into_iter()
        .map(|x| x.into_iter().map(|x| x.to_string()).collect())
        .collect();
    let mut tx = db.begin().await.context(error::DatabaseTransaction)?;
    for alias in tags {
        let id = tag::id_by_alias_match(&alias, &mut tx).await?;

        // TODO: use upsert
        if let Some(id) = id {
            tag::update_unique(id, &alias, &mut tx).await?;
        } else {
            query!(
                "
                insert into pixiv_tag (alias) values ($1)
                ",
                &alias
            )
            .execute(&mut tx)
            .await
            .context(error::DatabaseTransaction)?;
        };
    }
    tx.commit().await.context(error::DatabaseTransaction)?;

    Ok(())
}

pub async fn update_user_id_set(
    users_need_update_set: BTreeSet<String>,
    kit: &PixivKit,
) -> Result<()> {
    let need_sleep = users_need_update_set.len() > kit.config.pixiv.user_update_sleep_threshold;
    // Sleep for 500ms to avoid 403 error
    for user_id in users_need_update_set {
        try_skip!(update_user_detail(&user_id, kit).await);
        if need_sleep {
            tokio::time::sleep(kit.config.pixiv.user_update_sleep_interval).await;
        }
    }
    Ok(())
}

async fn update_user_detail(user_id: &str, kit: &PixivKit) -> Result<()> {
    info!("updating pixiv user data: {}", user_id);
    let resp = kit
        .api
        .user_detail(user_id)
        .await
        .context(error::PixivApi)?;

    let user = resp.user;
    let profile = resp.profile;
    let mut tx = kit.db.begin().await.context(error::DatabaseTransaction)?;

    let item_id = user::update_item_returning_id(user_id, &user, &profile, &mut tx).await?;

    macro_rules! not_empty {
        () => {
            |x| !x.is_empty()
        };
    }

    let mut workspace = resp.workspace;
    let workspace_image_url_string = workspace
        .remove("workspace_image_url")
        .unwrap_or_default()
        .filter(not_empty!());
    let workspace_image_url = workspace_image_url_string.as_deref();
    let avatar_url = Some(user.profile_image_urls.medium.as_str()).filter(not_empty!());
    let background_url = profile.background_image_url.as_deref().filter(not_empty!());

    media::insert_urls(
        [workspace_image_url, avatar_url, background_url]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .as_slice(),
        &mut tx,
    )
    .await?;

    user::update_history(
        item_id,
        &user,
        &profile,
        workspace,
        workspace_image_url,
        avatar_url,
        background_url,
        &mut tx,
    )
    .await?;

    tx.commit().await.context(error::DatabaseTransaction)?;

    if let Some(avatar_url) = avatar_url {
        download_other_image("avatar", avatar_url, kit).await?;
    }
    if let Some(background_url) = background_url {
        download_other_image("background", background_url, kit).await?;
    }
    if let Some(workspace_image_url) = workspace_image_url {
        download_other_image("workspace", workspace_image_url, kit).await?;
    }

    Ok(())
}

pub async fn save_image(
    db: &PgPool,
    size: i32,
    img_metadata: Option<ImageMetadata>,
    url: String,
    path: impl AsRef<Path>,
    path_db: String,
) -> Result<()> {
    let mime = mime_guess::from_path(path).first().map(|x| x.to_string());
    let mut tx = db.begin().await.context(error::DatabaseTransaction)?;

    let w: Option<i32> = img_metadata.as_ref().and_then(|x| x.width.try_into().ok());
    let h: Option<i32> = img_metadata.as_ref().and_then(|x| x.height.try_into().ok());
    let id =
        media::update_returning_id(&url, size, mime.as_deref(), &path_db, w, h, &mut tx).await?;

    if let Some(img_metadata) = img_metadata {
        media::insert_colors(id, &img_metadata.hsv_palette, &mut tx).await?;
    }

    tx.commit().await.context(error::DatabaseTransaction)?;
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
    let mut tx = db.begin().await.context(error::DatabaseTransaction)?;
    media::insert_ugoira(&zip_url, &zip_path_db, zip_size, &mut tx).await?;

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

        media::insert_ugoira_mp4(&mp4_path_db, size, &mut tx).await?;
    }
    tx.commit().await.context(error::DatabaseTransaction)?;
    Ok(())
}

pub async fn save_illusts(
    illusts: &Vec<pixivcrab::models::illust::Illust>,
    kit: &PixivKit,
    on_user_need_update: impl FnMut(&str),
    mut on_ugoira_metadata: impl FnMut(&str, (&str, &[i32])),
) -> Result<()> {
    preprocess_items!(
        illusts,
        &kit.db,
        kit.config.pixiv.user_need_update_interval,
        on_user_need_update
    );

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

        let mut tx = kit.db.begin().await.context(error::DatabaseTransaction)?;
        if !i.visible {
            if i.id != 0 {
                warn!("pixiv: Works {} is invisible!", id);
                set_source_inaccessible("pixiv_illust", &id, &mut tx).await?;
            }
            continue;
        }

        let item_id = illust::upsert_item_returning_id(i, &mut tx).await?;

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
        let urls_str = urls.iter().map(|x| x.as_str()).collect::<Vec<_>>();

        media::insert_urls(&urls_str, &mut tx).await?;
        if let Some(history_id) =
            illust::insert_history_returning_id(item_id, i, &urls, delay_slice, &mut tx).await?
        {
            illust::insert_history_media(history_id, &urls, &mut tx).await?;
        }

        tx.commit().await.context(error::DatabaseTransaction)?;
    }
    Ok(())
}

pub async fn save_novels(
    novels: &Vec<pixivcrab::models::novel::Novel>,
    update_exists: bool,
    kit: &PixivKit,
    mut on_each_should_continue: impl FnMut() -> bool,
    on_user_need_update: impl FnMut(&str),
) -> Result<()> {
    preprocess_items!(
        novels,
        &kit.db,
        kit.config.pixiv.user_need_update_interval,
        on_user_need_update
    );

    for n in novels {
        let id = n.id.to_string();
        info!("pixiv: getting novel text of {}", id);
        let r = kit.api.novel_text(&id).await.context(error::PixivApi)?;

        let mut tx = kit.db.begin().await.context(error::DatabaseTransaction)?;
        if !on_each_should_continue() {
            return Ok(());
        }
        if !n.visible {
            if n.id != 0 {
                set_source_inaccessible("pixiv_novel", &id, &mut tx).await?;
            }
            continue;
        }

        let item_id = novel::upsert_item_returning_id(n, &mut tx).await?;

        let history_exists = novel::history_exists(item_id, &mut tx).await?;
        if history_exists && !update_exists {
            continue;
        }

        novel::insert_history(item_id, n, &r.novel_text, &mut tx).await?;
        tx.commit().await.context(error::DatabaseTransaction)?;
    }

    Ok(())
}
