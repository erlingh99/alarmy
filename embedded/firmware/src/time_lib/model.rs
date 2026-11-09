use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct ApiTimeType {
    pub timestamp_ms: u64,
    pub utc_offset_seconds: i32,
}
