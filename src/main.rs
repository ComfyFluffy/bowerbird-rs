use std::process;
mod log;

#[tokio::main]
async fn main() {
    if let Err(e) = log::init_log4rs() {
        eprintln!("log4rs init error: {}", e);
        process::exit(1);
    }
    // TODO: add more info log

    // let mut agent = PyroscopeAgent::builder("http://localhost:4040", "bb")
    //     .build()
    //     .unwrap();
    // agent.start();

    process::exit(bowerbird::cli::run().await);
}
