use crate::{
    log::debug,
    models::{
        pixiv::{PixivIllustHistory, PixivWorks},
        Item,
    },
};

use error::ServerErrorExt;
use futures::TryStreamExt;
use mongodb::Database;
use rocket::{http::Status, routes, serde::json::Json, State};
use serde_json::Value;
use snafu::ResultExt;

use self::error::ErrorResponse;

mod error;

#[rocket::get("/")]
fn hello() -> &'static str {
    "Hello, world!"
}
type PIllust = Item<PixivWorks, PixivIllustHistory>;

#[rocket::post("/pixiv/find", data = "<filter>")]
async fn find_pixiv(
    filter: Json<Value>,
    db: &State<Database>,
) -> Result<Json<Vec<PIllust>>, ErrorResponse> {
    let doc = bson::to_document(&filter.0)
        .context(error::BsonSerialize)
        .with_status(Status::BadRequest)?;
    debug!("{}", doc);
    let mut cur = db
        .collection::<PIllust>("pixiv_illust")
        .find(doc, None)
        .await
        .context(error::MongoDB)
        .with_status(Status::BadRequest)?;
    let mut r = Vec::new();
    let mut sent = 1;
    while let Some(i) = cur
        .try_next()
        .await
        .context(error::MongoDB)
        .with_status(Status::InternalServerError)?
    {
        r.push(i);
        sent += 1;
        if sent >= 1000 {
            break;
        }
    }
    Ok(Json(r))
}

pub async fn run(db: Database) -> crate::Result<()> {
    rocket::build()
        .mount("/", routes![hello, find_pixiv])
        .manage(db)
        .launch()
        .await
        .context(error::Rocket)
}
