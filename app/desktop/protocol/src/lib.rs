//! Wire and domain contracts. This crate deliberately has no OS or storage dependency.
use serde::{Deserialize, Serialize};
use serde_json::Value;
pub mod report;
pub mod preferences;

pub type Result<T> = std::result::Result<T, String>;
pub const PROTOCOL_VERSION: u32 = 2;
pub const MAX_MESSAGE: usize = 1_000_000;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub protocol: u32,
    pub profile_id: String,
    pub request_id: String,
    pub method: String,
    pub params: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Response {
    pub protocol: u32,
    pub profile_id: String,
    pub agent_instance_id: String,
    pub result: Option<Value>,
    pub error: Option<String>,
}

pub fn now() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs_f64()
}

pub fn canonical(value: &Value) -> String {
    fn sort(value: &Value) -> Value {
        match value {
            Value::Object(map) => Value::Object(map.iter().map(|(k,v)|(k.clone(),sort(v))).collect()),
            Value::Array(items) => Value::Array(items.iter().map(sort).collect()),
            other => other.clone(),
        }
    }
    sort(value).to_string()
}

pub fn hash(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

pub fn secret_eq(a: &str, b: &str) -> bool {
    use sha2::{Digest, Sha256};
    Sha256::digest(a.as_bytes()).iter().zip(Sha256::digest(b.as_bytes())).fold(0u8,|n,(a,b)|n|(a^b)) == 0
}
