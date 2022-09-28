use actix_web::{
    get,
    http::header::{self, CacheDirective, ContentType},
    post,
    web::{self, Data, Json},
    HttpRequest, HttpResponse,
};
use bowerbird_core::{
    config::Config,
    model::{pixiv::PixivIllust, Image, Media, Tag},
};
use chrono::{DateTime, Utc};
use log::debug;
use serde::{Deserialize, Serialize};
use sqlx::{query_as, PgPool};
use std::sync::Mutex;
use tokio::sync::Semaphore;

use super::{
    error::*,
    utils::{cached_image_thumbnail, ThumbnailCache},
    PixivConfig, Result,
};

type OptionUtc = Option<DateTime<Utc>>;

#[derive(Debug, Clone, Deserialize)]
struct ThumbnailQuery {
    size: u32,
    crop_to_center: bool,
}
#[get("/thumbnail/{path:.*}")]
async fn thumbnail(
    req: HttpRequest,
    path: web::Path<(String,)>,
    query: web::Query<ThumbnailQuery>,
    config: Data<Config>,
    pixiv_config: Data<PixivConfig>,
    cache: Data<Mutex<ThumbnailCache>>,
    semaphore: Data<Semaphore>,
) -> Result<HttpResponse> {
    if req.headers().get(header::RANGE).is_some() {
        return Ok(HttpResponse::NotImplemented().finish());
    }
    let path = pixiv_config
        .storage_dir
        .join(path.0.replace("../", "").replace("..\\", ""));

    let img = cached_image_thumbnail(
        path,
        query.size,
        cache.as_ref(),
        semaphore.as_ref(),
        config.server.thumbnail_jpeg_quality,
        if query.crop_to_center {
            Some(0.75)
        } else {
            None
        },
    )
    .await?;

    Ok(HttpResponse::Ok()
        .content_type(ContentType::jpeg())
        .append_header(header::CacheControl(vec![CacheDirective::MaxAge(604800)]))
        .body(img))
}

#[derive(Debug, Clone, Deserialize)]
struct FindImageMediaForm {
    h_range: Option<(f32, f32)>,
    min_s: Option<f32>,
    min_v: Option<f32>,
}
#[post("/find/media/image")]
async fn find_image_media(
    db: Data<PgPool>,
    form: Json<FindImageMediaForm>,
) -> Result<Json<Vec<Media<Image>>>> {
    let h_range = form.h_range.unwrap_or((0.0, 360.0));
    let r = query_as(
        "
        select id, url, size, mime, local_path, width, height
        from pixiv_media
        where id in (
            select media_id from pixiv_media_color
            where ($1 <= $2 and (h >= $1 and h <= $2)) or ($1 > $2 and (h >= $1 or h <= $2))
            and (($3 is not null and s >= $3) or ($3 is null and s >= 0.2))
            and (($4 is not null and v >= $4) or ($4 is null and v >= 0.2))
        ) order by id desc
        ",
    )
    .bind(h_range.0)
    .bind(h_range.1)
    .bind(form.min_s)
    .bind(form.min_v)
    .fetch_all(db.as_ref())
    .await
    .with_interal()?;

    Ok(Json(r))
}

#[derive(Debug, Clone, Deserialize)]
struct FindTagForm {
    search: String,
    limit: u16,
    offset: u16,
}
#[post("/find/tag/search")]
async fn find_tag_search(db: Data<PgPool>, form: Json<FindTagForm>) -> Result<Json<Vec<Tag>>> {
    let form = form.into_inner();

    let r = query_as(
        "
        select alias, id
        from pixiv_tag
        where id in (select distinct id
             from (select id, unnest(alias) tag
                   from pixiv_tag) x
             where tag like $1)
        limit $2 offset $3
        ",
    )
    .bind(format!("%{}%", form.search))
    .bind(form.limit as i32)
    .bind(form.offset as i32)
    .fetch_all(db.as_ref())
    .await
    .with_interal()?;
    Ok(Json(r))
}

#[derive(Debug, Clone, Serialize)]
struct ItemsResponse<T> {
    pub items: Vec<T>,
    pub total: i64,
}

#[derive(Debug, Clone, Deserialize)]
struct FindIllustForm {
    tags: Option<Vec<i32>>,
    tags_exclude: Option<Vec<i32>>,
    ids: Option<Vec<i32>>,
    search: Option<String>, // Search in title and caption
    date_range: Option<(OptionUtc, OptionUtc)>,
    bookmark_range: Option<(Option<u16>, Option<u16>)>, // (min, max)
    parent_ids: Option<Vec<i32>>,
    limit: u16,
    offset: u16,
}
#[post("/find/illust")]
async fn find_illust(
    db: Data<PgPool>,
    form: Json<FindIllustForm>,
) -> Result<Json<ItemsResponse<PixivIllust>>> {
    let form = form.into_inner();
    debug!("find illust: {:?}", form);

    let r: Vec<PixivIllust> = query_as(
        // join the latest history from pixiv_illust_history and apply the filter.
        "
        select count(*) over() _count,
               pi.id id,
               pi.parent_id parent_id,
               pih.id history_id,
               source_id,
               source_inaccessible,
               total_bookmarks,
               total_view,
               is_bookmarked,
               tag_ids,
               illust_type,
               caption_html,
               title,
               date,
               media_ids,
               (select array_agg(local_path)
                from pixiv_media pm
                where pm.id = any (pih.media_ids)
                group by pih.id) image_paths
                
        from pixiv_illust_history pih
                 join (select max(id) id, item_id from pixiv_illust_history group by item_id) sub using (id, item_id)
                 join pixiv_illust pi on pi.id = pih.item_id

        where 
            ($1 is null or pi.id = any($1))
            and ($8::varchar[] is null or tag_ids @> $8)
            and ($9::varchar[] is null or not tag_ids && $9)
            and ($7 is null or array_length($7, 1) is null or parent_id = any($7))
            and ($3 is null or date >= $3)
            and ($4 is null or date <= $4)
            and ($5 is null or total_bookmarks >= $5)
            and ($6 is null or total_bookmarks <= $6)
            and ($2::text is null or title ilike $2 or caption_html ilike $2)
        
        order by pi.id desc
        limit $10 offset $11
        ",
    )
    .bind(form.ids)
    .bind(form.search.map(|s| format!("%{}%", s)))
    .bind(form.date_range.and_then(|(a, _)| a))
    .bind(form.date_range.and_then(|(_, b)| b))
    .bind(form.bookmark_range.and_then(|(a, _)| a.map(|x|x as i32)))
    .bind(form.bookmark_range.and_then(|(_, b)| b.map(|x|x as i32)))
    .bind(form.parent_ids)
    .bind(form.tags)
    .bind(form.tags_exclude)
    .bind(
        form.limit as i32,
    ).bind(
        form.offset as i32,
    ).fetch_all(db.as_ref()).await.with_interal()?;

    Ok(Json(ItemsResponse {
        total: r.first().and_then(|x| x._count).unwrap_or(0),
        items: r,
    }))
}

// #[derive(Debug, Clone, Deserialize)]
// struct FindUserForm {
//     ids: Option<Vec<i32>>,
//     search: Option<String>, // Search in name
//     limit: u16,
//     offset: u16,
// }
// #[post("/find/user")]
// async fn find_user(db: Data<PgPool>, form: Json<FindUserForm>) -> Result<Json<Vec<PixivUser>>> {
//     let form = form.into_inner();
//     debug!("find user: {:?}", form);

//     let r = query_as(
//         "
//         select id, name, account, profile_image_urls, is_followed
//         from pixiv_user
//         where ($1 is null or array_length($1, 1) is null or id = any($1))
//         and ($2 is null or name ilike $2)
//         order by id desc
//         limit $3 offset $4
//         ",
//     )
//     .bind(form.ids)
//     .bind(form.search.map(|s| format!("%{}%", s)))
//     .bind(form.limit as i32)
//     .bind(form.offset as i32)
//     .fetch_all(db.as_ref())
//     .await
//     .with_interal()?;

//     Ok(Json(r))
// }
