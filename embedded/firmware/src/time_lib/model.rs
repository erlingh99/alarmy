use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct ApiTimeType {
    pub timestamp: u64,
    pub utc_offset_seconds: i32,
}
