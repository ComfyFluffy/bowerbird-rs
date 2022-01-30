use rbatis::{crud::CRUD, rbatis::Rbatis};

pub async fn migrate(db: mongodb::Database) {
    let rb = Rbatis::new();
    rb.link(&std::env::var("MY").unwrap()).await.unwrap();
    rb.save(table, skips)
}
