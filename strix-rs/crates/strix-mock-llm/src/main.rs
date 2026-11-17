//! Binary wrapper around the mock `llama-server`.
//!
//! The integration tests start the server in-process; this binary exists so a
//! developer (or CI) can run the same mock by hand:
//!
//! ```text
//! cargo run -p strix-mock-llm -- --port 8080 --model qwen3-coder-30b
//! ```

use anyhow::Result;
use strix_mock_llm::AppState;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let mut args = std::env::args().skip(1);
    let mut host = "127.0.0.1".to_owned();
    let mut port: u16 = 0;
    let mut model = "mock-model".to_owned();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--host" => {
                if let Some(value) = args.next() {
                    host = value;
                }
            }
            "--port" => {
                if let Some(value) = args.next() {
                    port = value.parse().unwrap_or(0);
                }
            }
            "--model" => {
                if let Some(value) = args.next() {
                    model = value;
                }
            }
            other => {
                eprintln!("unknown argument: {other}");
            }
        }
    }

    let app = strix_mock_llm::router(AppState::new(&model));
    let listener = TcpListener::bind((host.as_str(), port)).await?;
    let addr = listener.local_addr()?;
    println!("mock llama-server listening on http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
