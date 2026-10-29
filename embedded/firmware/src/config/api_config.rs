/// Network configuration constants and structures
macro_rules! api_route {
    ($path:literal) => {
        concat!("https://alarmy.lan/api/", $path)
    };
}

pub const ALARM_SYNC_ROUTE: &str = api_route!("alarms/sync");
pub const TIME_ROUTE: &str = api_route!("time");
pub const HEALTH_ROUTE: &str = api_route!("health");
pub const LOG_ROUTE: &str = api_route!("logs");
pub const FIRMWARE_VERSION_ROUTE: &str = api_route!("firmware/version");
pub const FIRMWARE_BINARY_ROUTE: &str = api_route!("firmware/binary");

pub const RETRY_DELAY_S: u64 = 5; // Delay in seconds before retrying network operations
pub const API_POLLING_INTERVAL_S: u64 = 3600; // Interval for polling the API for new alarms
pub const API_RETRY_S: u64 = 30;
pub const TIME_SYNC_INTERVAL_S: u64 = 3600; // Interval for synchronizing time]
