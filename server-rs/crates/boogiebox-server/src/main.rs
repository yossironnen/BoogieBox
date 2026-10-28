//! Defines Rust server support logic for Main.

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(code) = boogiebox_server::backup::run_cli(&args) {
        std::process::exit(code);
    }
    if let Err(error) = boogiebox_server::run_from_env().await {
        eprintln!("FATAL startup error: {error}");
        std::process::exit(1);
    }
}
