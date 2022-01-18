use rocket::{
    response::{self, Responder},
    Request, Response,
};

pub struct CachedResponse(pub Vec<u8>);
impl<'r> Responder<'r, 'static> for CachedResponse {
    fn respond_to(self, r: &'r Request<'_>) -> response::Result<'static> {
        Response::build()
            .join(self.0.respond_to(r)?)
            .raw_header("cache-control", "max-age=31536000")
            .ok()
    }
}
