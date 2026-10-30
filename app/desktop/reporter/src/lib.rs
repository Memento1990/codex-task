pub mod store;
pub mod timing;
pub mod runtime;
pub mod ingest;
pub mod integration;
pub mod desktop;

pub type Result<T> = std::result::Result<T, String>;

pub fn now() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs_f64()
}

pub fn data_root() -> std::path::PathBuf {
    let args: Vec<String> = std::env::args().collect();
    args.windows(2).find(|a| a[0] == "--data-dir").map(|a| a[1].clone()).or_else(|| std::env::var("CODEX_FLOATBOARD_DATA").ok()).map(std::path::PathBuf::from).unwrap_or_else(|| std::path::PathBuf::from(std::env::var("LOCALAPPDATA").unwrap_or_default()).join("CodexFloatboard/data"))
}
