//! Native shell and signed delivery. No database dependency.
pub mod update;
pub mod update_install;
pub mod build_identity;
pub mod weixin_lab;
pub mod paths;
pub mod file_lock;
pub mod install;
pub mod legacy;
pub use floatboard_protocol::{now,Result};
