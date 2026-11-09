//! Local IPC and Windows identity. No SQLite or business-store dependency.
pub mod windows;
pub mod scheduled;
pub mod profile;
pub mod ipc;
pub mod files;
pub mod notices;
pub mod desktop;
pub use floatboard_protocol::{now, Result};
pub use profile::Profile;
pub use ipc::Client;

pub mod build_identity;
