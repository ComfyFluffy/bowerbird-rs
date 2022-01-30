use std::convert::Infallible;

use dav_server::{memls::MemLs, DavHandler, memfs::MemFs};

mod fs;
mod query;

pub async fn run() {
    let dav_server = DavHandler::builder()
        .filesystem(MemFs::new())
        .locksystem(MemLs::new())
        .build_handler();

    let make_service = hyper::service::make_service_fn(move |_| {
        let dav_server = dav_server.clone();
        async move {
            let func = move |req| {
                let dav_server = dav_server.clone();
                async move { Ok::<_, Infallible>(dav_server.handle(req).await) }
            };
            Ok::<_, Infallible>(hyper::service::service_fn(func))
        }
    });
    hyper::Server::bind(&([127, 0, 0, 1], 4918).into())
        .serve(make_service)
        .await
        .unwrap();
}
