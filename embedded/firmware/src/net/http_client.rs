use embassy_net::dns::DnsSocket;
use embassy_net::tcp::client::{TcpClient, TcpClientState};
use embassy_net::Stack;
use embassy_rp::clocks::RoscRng;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::{Mutex, MutexGuard};
use embassy_time::Duration;
use embedded_io_async::Read as _;
use heapless::Vec;
use reqwless::client::{HttpClient, TlsConfig, TlsVerify};
use reqwless::headers::ContentType;
use reqwless::request::{Method, RequestBuilder};
use serde::Deserialize;

use crate::alarm::model::Alarm;
use crate::config::api_config::{
    ALARM_SYNC_ROUTE, FIRMWARE_VERSION_ROUTE, HEALTH_ROUTE, LOG_ROUTE, TIME_ROUTE,
};
use crate::ota::VersionResponse;
use crate::time_lib::model::ApiTimeType;

/// Device-facing payload from the backend. The arming flag travels with the
/// alarms so there is no window on boot where the device holds alarms but not
/// yet the arming state.
#[derive(Deserialize, Debug)]
pub struct AlarmSync {
    pub alarms: Vec<Alarm, 8>,
    pub alarms_enabled: bool,
}

#[derive(Debug)]
pub enum NetworkError {
    BackendDown,
    RequestFailed,
    ReadError,
    ParseError,
    FlashError,
    TcpError,
}

#[derive(Deserialize)]
pub enum HealthStatus {
    #[serde(alias = "ok")]
    Ok,
    #[serde(alias = "not_ok")]
    NotOk,
}

#[derive(Deserialize)]
pub struct HealthResponse {
    status: HealthStatus,
}

struct HttpBuffers {
    tls_read: [u8; 16640],
    tls_write: [u8; 16640],
    rx: [u8; 8192],
}

pub struct HttpClientManager {
    stack: &'static Stack<'static>,
    http_mutex: Mutex<CriticalSectionRawMutex, HttpBuffers>,
}

impl HttpClientManager {
    pub fn new(stack: &'static Stack<'static>) -> Self {
        Self {
            stack,
            http_mutex: Mutex::new(HttpBuffers {
                tls_read: [0; 16640],
                tls_write: [0; 16640],
                rx: [0; 8192],
            }),
        }
    }

    /// No logging in this method as the log module uses it to send logs creating infinite logging loop
    async fn make_http_request(
        &self,
        method: Method,
        url: &str,
        msg: Option<&[u8]>,
    ) -> Result<(MutexGuard<'_, CriticalSectionRawMutex, HttpBuffers>, usize), NetworkError> {
        let mut guard = self.http_mutex.lock().await;
        let len = {
            let buf = &mut *guard;
            let client_state = TcpClientState::<1, 1024, 4096>::new();
            let mut tcp_client = TcpClient::new(*self.stack, &client_state);
            tcp_client.set_timeout(Some(Duration::from_secs(20)));
            let dns_client = DnsSocket::new(*self.stack);
            let mut rng = RoscRng;
            let seed = rng.next_u64();
            let tls_config =
                TlsConfig::new(seed, &mut buf.tls_read, &mut buf.tls_write, TlsVerify::None);

            let mut http_client = HttpClient::new_with_tls(&tcp_client, &dns_client, tls_config);
            let request = http_client
                .request(method, url)
                .await
                .map_err(|_| NetworkError::RequestFailed)?;

            let content = msg.unwrap_or_default();
            let mut request_with_body = request
                .body(content)
                .content_type(ContentType::ApplicationJson);

            let buf_start = buf.rx.as_ptr() as usize;
            let rx_size = buf.rx.len();
            let response = request_with_body
                .send(&mut buf.rx[..rx_size])
                .await
                .map_err(|_| NetworkError::RequestFailed)?;

            let body_bytes = response
                .body()
                .read_to_end()
                .await
                .map_err(|_| NetworkError::ReadError)?;

            core::str::from_utf8(body_bytes).map_err(|_| NetworkError::ParseError)?;
            let offset = body_bytes.as_ptr() as usize - buf_start;
            let len = body_bytes.len();
            buf.rx.copy_within(offset..offset + len, 0);
            len
            // request_with_body, http_client, tcp_client, etc. dropped here,
            // releasing all borrows on guard before it is moved out below
        };
        Ok((guard, len))
    }

    /// Stream a GET response body, calling `handler` for each chunk of bytes received.
    /// Uses a 120 s timeout suited for large binary downloads.
    pub(crate) async fn stream_response(
        &self,
        url: &str,
        mut handler: impl FnMut(&[u8]) -> Result<(), NetworkError>,
    ) -> Result<usize, NetworkError> {
        let mut guard = self.http_mutex.lock().await;
        let client_state = TcpClientState::<1, 1024, 4096>::new();
        let mut tcp = TcpClient::new(*self.stack, &client_state);
        tcp.set_timeout(Some(Duration::from_secs(120)));
        let dns = DnsSocket::new(*self.stack);
        let mut rng = RoscRng;
        let seed = rng.next_u64();
        let buffers = &mut *guard;
        let tls_config = TlsConfig::new(
            seed,
            &mut buffers.tls_read,
            &mut buffers.tls_write,
            TlsVerify::None,
        );
        let mut http = HttpClient::new_with_tls(&tcp, &dns, tls_config);

        let mut req = http
            .request(Method::GET, url)
            .await
            .map_err(|_| NetworkError::RequestFailed)?;

        let mut header_buf = [0u8; 1024];
        let response = req
            .send(&mut header_buf)
            .await
            .map_err(|_| NetworkError::RequestFailed)?;

        if !response.status.is_successful() {
            log::error!("HTTP request failed with status {:#?}", response.status);
            return Err(NetworkError::RequestFailed);
        }

        let mut body = response.body().reader();
        let mut total = 0usize;

        loop {
            match body.read(&mut buffers.rx).await {
                Ok(0) => break,
                Ok(n) => {
                    total += n;
                    handler(&buffers.rx[..n])?;
                }
                Err(_) => return Err(NetworkError::ReadError),
            }
        }

        Ok(total)
    }

    pub async fn get_alarm_sync(&self) -> Result<AlarmSync, NetworkError> {
        log::debug!("Making HTTP request to {ALARM_SYNC_ROUTE}");
        let (guard, len) = self
            .make_http_request(Method::GET, ALARM_SYNC_ROUTE, None)
            .await?;
        serde_json_core::de::from_slice::<AlarmSync>(&guard.rx[..len])
            .map(|(v, _)| v)
            .map_err(|e| {
                log::error!("Failed to parse alarm sync JSON: {e}");
                NetworkError::ParseError
            })
    }

    pub async fn get_time(&self) -> Result<ApiTimeType, NetworkError> {
        log::debug!("Making HTTP request to {TIME_ROUTE}");
        let (guard, len) = self
            .make_http_request(Method::GET, TIME_ROUTE, None)
            .await?;
        serde_json_core::de::from_slice::<ApiTimeType>(&guard.rx[..len])
            .map(|(v, _)| v)
            .map_err(|_| {
                log::error!("Failed to parse time response");
                NetworkError::ParseError
            })
    }

    pub async fn get_health(&self) -> Result<(), NetworkError> {
        log::debug!("Making HTTP request to {HEALTH_ROUTE}");
        let (guard, len) = self
            .make_http_request(Method::GET, HEALTH_ROUTE, None)
            .await?;
        let health = serde_json_core::de::from_slice::<HealthResponse>(&guard.rx[..len])
            .map(|(v, _)| v.status)
            .map_err(|_| {
                log::error!("Failed to parse health response");
                NetworkError::ParseError
            })?;
        match health {
            HealthStatus::Ok => Ok(()),
            _ => Err(NetworkError::BackendDown),
        }
    }

    pub async fn get_firmware_version(&self) -> Result<VersionResponse, NetworkError> {
        log::debug!("Getting remote firmware version");
        let (guard, len) = self
            .make_http_request(Method::GET, FIRMWARE_VERSION_ROUTE, None)
            .await?;
        serde_json_core::de::from_slice::<VersionResponse>(&guard.rx[..len])
            .map(|(v, _)| v)
            .map_err(|_| {
                log::error!("Failed to parse firmware version response");
                NetworkError::ParseError
            })
    }

    pub async fn post_log(&self, msg: &[u8]) -> Result<(), NetworkError> {
        // No logging allowed in this method
        let _ = self
            .make_http_request(Method::POST, LOG_ROUTE, Some(msg))
            .await?;
        Ok(())
    }
}
