use bson::{doc, to_bson};
use futures::TryStreamExt;
use mongodb::Database;
use serde::Deserialize;
use snafu::ResultExt;

use crate::{
    error,
    log::info,
    model::{BowerbirdMetadata, Hsv, LocalMedia},
    utils::rgb_to_hsv,
};

pub const DB_VERSION: i32 = 2;

async fn update_version(db: &Database, version: i32) -> crate::Result<()> {
    db.collection::<BowerbirdMetadata>("bowerbird_metadata")
        .update_one(
            doc! {},
            doc! {
                "$set": {
                    "version": version,
                }
            },
            None,
        )
        .await
        .context(error::MongoDb)?;
    Ok(())
}

async fn operations(db: &Database, target_version: i32) -> crate::Result<()> {
    match target_version {
        2 => {
            #[derive(Deserialize)]
            struct Rgb(i16, i16, i16);

            #[derive(Deserialize)]
            struct ImageMedia {
                palette_rgb: Vec<Rgb>,
            }

            type Media = LocalMedia<ImageMedia>;

            fn i16_to_u8(i: &i16) -> u8 {
                (*i).try_into().unwrap_or_default()
            }

            let c_image = db.collection::<Media>("pixiv_image");
            let mut cur = c_image
                .find(
                    doc! {
                        "extension.palette_rgb": {
                            "$exists": true,
                        }
                    },
                    None,
                )
                .await
                .context(error::MongoDb)?;
            while let Some(r) = cur.try_next().await.context(error::MongoDb)? {
                let hsv_v: Vec<Hsv> = r
                    .extension
                    .unwrap()
                    .palette_rgb
                    .iter()
                    .map(|Rgb(r, g, b)| {
                        let (h, s, v) = rgb_to_hsv(i16_to_u8(r), i16_to_u8(g), i16_to_u8(b));
                        Hsv { h, s, v }
                    })
                    .collect();
                c_image
                    .update_one(
                        doc! {"_id": r._id},
                        doc! {
                        "$set": { "extension.palette_hsv": to_bson(&hsv_v).unwrap() },
                        "$unset": { "extension.palette_rgb": "" }},
                        None,
                    )
                    .await
                    .context(error::MongoDb)?;
            }
            update_version(db, 2).await?;
        }
        _ => {
            panic!("Unknown target version: {}", target_version);
        }
    }
    Ok(())
}

pub async fn migrate(db: &Database) -> crate::Result<()> {
    if let Some(metadata) = get_metadata(db).await? {
        if metadata.version < DB_VERSION {
            for version in metadata.version + 1..=DB_VERSION {
                info!("Migrating to version {}", version);
                operations(db, version).await?;
            }
        }
    }
    Ok(())
}

pub async fn get_metadata(db: &Database) -> crate::Result<Option<BowerbirdMetadata>> {
    db.collection::<BowerbirdMetadata>("bowerbird_metadata")
        .find_one(None, None)
        .await
        .context(error::MongoDb)
}
