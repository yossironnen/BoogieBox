//! Defines Rust server support logic for Main.

#[tokio::main]
async fn main() {
    if let Err(error) = boogiebox_server::run_from_env().await {
        eprintln!("FATAL startup error: {error}");
        std::process::exit(1);
    }
}
