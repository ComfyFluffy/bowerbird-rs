use std::process;

use bowerbird::error;

#[tokio::main]
async fn main() {
    match bowerbird::cli::run().await {
        Err(e) => {
            error!("{:?}", e);
            process::exit(1);
        }
        _ => {}
    };
}
