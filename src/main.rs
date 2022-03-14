#[tokio::main]
async fn main() {
    std::env::set_var("RUST_LOG", "debug");
    std::env::set_var("RUST_BACKTRACE", "1");
    env_logger::init();

    // let mut agent = PyroscopeAgent::builder("http://localhost:4040", "bb")
    //     .build()
    //     .unwrap();
    // agent.start();

    bowerbird::cli::run().await;
}
