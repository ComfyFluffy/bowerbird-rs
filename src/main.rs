#[tokio::main]
async fn main() -> i32 {
    // let mut agent = PyroscopeAgent::builder("http://localhost:4040", "bb")
    //     .build()
    //     .unwrap();
    // agent.start();

    dotenvy::dotenv().ok();

    bowerbird_cli::run().await
}
