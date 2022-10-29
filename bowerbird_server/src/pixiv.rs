use actix_web::{
    get,
    http::header::{self, CacheDirective, ContentType},
    post,
    web::{self, Data, Json},
    HttpRequest, HttpResponse,
};
use bowerbird_core::{
    config::Config,
    model::{
        pixiv::{PixivIllust, PixivUser},
        Item, Tag,
    },
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

fn count_items<E, H>(r: &[Item<E, H>]) -> i64 {
    r.first().and_then(|x| x._count).unwrap_or(0)
}

// TODO: Optimize offset: https://stackoverflow.com/questions/34110504/optimize-query-with-offset-on-large-table/34291099#34291099
#[derive(Debug, Clone, Deserialize)]
struct Cursor {
    limit: u16,
    offset: u16,
}

// #[derive(Debug, Clone, Deserialize)]
// struct FindImageMediaForm {
//     h_range: Option<(f32, f32)>,
//     min_s: Option<f32>,
//     min_v: Option<f32>,
// }
// #[post("/media/image/find")]
// async fn find_image_media(
//     db: Data<PgPool>,
//     form: Json<FindImageMediaForm>,
// ) -> Result<Json<Vec<Media<Image>>>> {
//     let h_range = form.h_range.unwrap_or((0.0, 360.0));
//     let r = query_as(
//         "
//         select id, url, size, mime, local_path, width, height
//         from pixiv_media
//         where id in (
//             select media_id from pixiv_media_color
//             where ($1 <= $2 and (h >= $1 and h <= $2)) or ($1 > $2 and (h >= $1 or h <= $2))
//             and (($3 is not null and s >= $3) or ($3 is null and s >= 0.2))
//             and (($4 is not null and v >= $4) or ($4 is null and v >= 0.2))
//         ) order by id desc
//         ",
//     )
//     .bind(h_range.0)
//     .bind(h_range.1)
//     .bind(form.min_s)
//     .bind(form.min_v)
//     .fetch_all(db.as_ref())
//     .await
//     .with_interal()?;

//     Ok(Json(r))
// }

#[derive(Debug, Clone, Deserialize)]
struct TagFindForm {
    ids: Option<Vec<i64>>,
    search: Option<String>,
    #[serde(flatten)]
    cursor: Cursor,
}
#[post("/tag/find")]
async fn find_tag(db: Data<PgPool>, form: Json<TagFindForm>) -> Result<Json<Vec<Tag>>> {
    // TODO: change return type
    let form = form.into_inner();

    let r = query_as(
        "
        select alias, id
        from pixiv_tag
        where ($1 is null or id = any ($1))
          and ($2 is null or id in (select distinct id
                                    from (select id, unnest(alias) tag
                                          from pixiv_tag) t
                                    where tag ilike $2))
        limit $3 offset $4
        ",
    )
    .bind(form.ids)
    .bind(
        form.search
            .filter(|v| !v.is_empty())
            .map(|v| format!("%{}%", v)),
    )
    .bind(form.cursor.limit as i64)
    .bind(form.cursor.offset as i64)
    .fetch_all(db.as_ref())
    .await
    .with_interal()?;
    Ok(Json(r))
}

#[derive(Debug, Clone, Serialize)]
struct ItemsResponse<T> {
    pub total: i64,
    pub items: Vec<T>,
}

#[derive(Debug, Clone, Deserialize)]
struct IllustFindForm {
    tag_ids: Option<Vec<i64>>,
    tag_ids_exclude: Option<Vec<i64>>,
    ids: Option<Vec<i64>>,
    search: Option<String>, // Search in title and caption
    date_range: Option<(OptionUtc, OptionUtc)>,
    bookmark_range: Option<(Option<u16>, Option<u16>)>, // (min, max)
    parent_ids: Option<Vec<i64>>,
    #[serde(flatten)]
    cursor: Cursor,
}
#[post("/illust/find")]
async fn find_illust(
    db: Data<PgPool>,
    form: Json<IllustFindForm>,
) -> Result<Json<ItemsResponse<PixivIllust>>> {
    let form = form.into_inner();
    debug!("find illust: {:?}", form);

    let r: Vec<PixivIllust> = query_as(
        // join the latest history from pixiv_illust_history and apply the filter.
        // https://stackoverflow.com/questions/8276383/postgresql-join-to-most-recent-record-between-tables
        "
        select count(*) over() _count,
               i.id id,
               i.parent_id parent_id,
               h.id history_id,
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
                from pixiv_media m
                where m.id = any (h.media_ids)
                group by h.id) image_paths
                
        from pixiv_illust_history h
                 join (select max(id) id, item_id from pixiv_illust_history group by item_id) sub using (id, item_id)
                 join pixiv_illust i on i.id = h.item_id
        
        where 
            ($1 is null or i.id = any($1))
            and ($8::varchar[] is null or tag_ids @> $8)
            and ($9::varchar[] is null or not tag_ids && $9)
            and ($7 is null or array_length($7, 1) is null or parent_id = any($7))
            and ($3 is null or date >= $3)
            and ($4 is null or date <= $4)
            and ($5 is null or total_bookmarks >= $5)
            and ($6 is null or total_bookmarks <= $6)
            and ($2::text is null or title ilike $2 or caption_html ilike $2)
        
        order by i.id desc
        limit $10 offset $11
        ",
    )
    .bind(form.ids)
    .bind(form.search.map(|s| format!("%{}%", s)))
    .bind(form.date_range.and_then(|(a, _)| a))
    .bind(form.date_range.and_then(|(_, b)| b))
    .bind(form.bookmark_range.and_then(|(a, _)| a.map(|x| x as i32)))
    .bind(form.bookmark_range.and_then(|(_, b)| b.map(|x| x as i32)))
    .bind(form.parent_ids)
    .bind(form.tag_ids)
    .bind(form.tag_ids_exclude)
    .bind(form.cursor.limit as i64)
    .bind(form.cursor.offset as i64)
    .fetch_all(db.as_ref())
    .await
    .with_interal()?;

    Ok(Json(ItemsResponse {
        total: count_items(&r),
        items: r,
    }))
}

#[derive(Debug, Clone, Deserialize)]
struct UserFindForm {
    ids: Option<Vec<i64>>,
    search: Option<String>, // Search in name
    #[serde(flatten)]
    cursor: Cursor,
}
#[post("/user/find")]
async fn find_user(
    db: Data<PgPool>,
    form: Json<UserFindForm>,
) -> Result<Json<ItemsResponse<PixivUser>>> {
    let form = form.into_inner();
    debug!("find user: {:?}", form);

    let r = query_as(
        "
        select count(*) over () _count, *
        from pixiv_illust_latest
        where
            ($1 is null or id = any($1))
            and ($2::text is null or name ilike $2)

        order by id desc
        limit $3 offset $4
        ",
    )
    .bind(form.ids)
    .bind(form.search.map(|s| format!("%{}%", s)))
    .bind(form.cursor.limit as i64)
    .bind(form.cursor.offset as i64)
    .fetch_all(db.as_ref())
    .await
    .with_interal()?;

    Ok(Json(ItemsResponse {
        total: count_items(&r),
        items: r,
    }))
}

// #[derive(Debug, Clone, Deserialize)]
// struct UserPreviewForm {
//     id: i32,
//     limit: u8,
// }

// Get comments with pixiv api by id
// #[derive(Debug, Clone, Deserialize)]
// struct GetCommentsForm {
//     id: i32,
//     page: Option<u16>,
// }
// #[post("/comment/byItemId")]
// async fn get_comments(
//     db: Data<PgPool>,
//     kit: Data<PixivKit>,
//     form: Json<GetCommentsForm>,
// ) -> Result<Json<Vec<PixivComment>>> {
//     let form = form.into_inner();
//     debug!("get comments: {:?}", form);
//     kit.api.send_authorized(request)

//     Ok(Json(r))
// }
