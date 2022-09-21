#[tokio::main]
async fn main() {
    // let mut agent = PyroscopeAgent::builder("http://localhost:4040", "bb")
    //     .build()
    //     .unwrap();
    // agent.start();

    dotenvy::dotenv().ok();

    std::process::exit(bowerbird_cli::run().await);
}
