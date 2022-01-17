mod aria2;
use futures::future::BoxFuture;

pub use aria2::Aria2Downloader;

use crate::error::BoxError;

pub struct Task {
    pub url: String,
    pub options: Option<aria2_ws::TaskOptions>,
    pub hooks: Option<TaskHooks>,
}

pub type BoxFutureResult = BoxFuture<'static, Result<(), BoxError>>;
#[derive(Default)]
pub struct TaskHooks {
    pub on_success: Option<BoxFutureResult>,
    pub on_error: Option<BoxFutureResult>,
}

fn print_option<T>(t: &Option<T>) -> &str {
    if t.is_some() {
        "Some(..)"
    } else {
        "None"
    }
}
impl std::fmt::Debug for TaskHooks {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_struct("TaskHooks")
            .field("on_success", &print_option(&self.on_success))
            .field("on_error", &print_option(&self.on_error))
            .finish()
    }
}
