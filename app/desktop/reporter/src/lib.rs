pub mod store;
pub mod timing;
pub mod runtime;
pub mod ingest;
pub mod integration;
pub mod desktop;
pub mod migration;
pub mod bridge;
pub mod update;
pub mod update_install;
pub mod build_identity;
pub mod notifications;
pub mod weixin_lab;

pub type Result<T> = std::result::Result<T, String>;

pub fn now() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs_f64()
}

pub fn data_root() -> std::path::PathBuf {
    let args: Vec<String> = std::env::args().collect();
    let selected = args.windows(2).find(|a| a[0] == "--data-dir").map(|a| a[1].clone()).or_else(|| std::env::var("CODEX_FLOATBOARD_DATA").ok()).map(std::path::PathBuf::from).unwrap_or_else(|| std::path::PathBuf::from(std::env::var("LOCALAPPDATA").unwrap_or_default()).join("CodexFloatboard/data"));
    // Fail closed: a broken redirect must never silently reopen the old database.
    migration::resolve_root(&selected).unwrap_or_else(|error| panic!("数据目录不可用: {error}"))
}
