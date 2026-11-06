use cyw43::{Control, JoinAuth, JoinOptions};
use embassy_executor::Spawner;
use embassy_futures::select::{Either, select};
use embassy_net::Stack;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::watch::{Receiver, Watch};
use embassy_time::Timer;

use super::http_client::HttpClient;
use crate::config::api_config::RETRY_DELAY_S;
use crate::config::wifi_config::WifiCredentials;
use crate::health::{self, Backoff, Fault};

/// Lets a fresh connection settle before the first health check
const FIRST_CHECK_DELAY_S: u64 = 300;
const CHECK_INTERVAL_S: u64 = 3600;
/// Consecutive failed health checks before rejoining
const MAX_FAILURES: u32 = 10;

/// The display
const SUBSCRIBERS: usize = 1;
static CONNECTED: Watch<CriticalSectionRawMutex, bool, SUBSCRIBERS> = Watch::new();

/// Receives whether WiFi is connected, each time that changes
pub fn subscribe() -> Receiver<'static, CriticalSectionRawMutex, bool, SUBSCRIBERS> {
    CONNECTED
        .receiver()
        .expect("more WiFi subscribers than SUBSCRIBERS allows")
}

/// Keeps WiFi connected, rejoining when the link drops or the backend stays unreachable
struct WifiActor {
    control: Control<'static>,
    stack: Stack<'static>,
    http_client: &'static HttpClient,
}

impl WifiActor {
    async fn run(&mut self) -> ! {
        let creds = load_credentials().await;
        let join_options = if creds.psk.is_empty() {
            JoinOptions::new_open()
        } else {
            // A prehashed PSK only works for WPA2 (WPA3/SAE needs the passphrase)
            let mut options = JoinOptions::new(&creds.psk);
            options.auth = JoinAuth::Wpa2;
            options.passphrase_is_prehashed = true;
            options
        };

        let mut backoff = Backoff::new();
        loop {
            match self.control.join(&creds.ssid, join_options.clone()).await {
                Ok(_) => {
                    log::info!("WiFi connected successfully");
                    health::clear(Fault::Wifi);
                    backoff.reset();
                    // The stack sees the link a moment after the join
                    self.stack.wait_link_up().await;
                    CONNECTED.sender().send(true);
                    match select(self.stack.wait_link_down(), self.wait_until_unreachable()).await {
                        Either::First(()) => health::report(Fault::Wifi, "link down"),
                        Either::Second(()) => log::warn!("Backend unreachable, rejoining WiFi"),
                    }
                    CONNECTED.sender().send(false);
                    self.control.leave().await;
                }
                Err(err) => {
                    CONNECTED.sender().send(false);
                    health::report(Fault::Wifi, err);
                }
            }
            backoff.wait().await;
            log::info!("Retrying WiFi connection...");
        }
    }

    async fn wait_until_unreachable(&self) {
        Timer::after_secs(FIRST_CHECK_DELAY_S).await;
        let mut failures = 0;
        loop {
            match self.http_client.get_health().await {
                Ok(()) => {
                    health::clear(Fault::Backend);
                    failures = 0;
                    Timer::after_secs(CHECK_INTERVAL_S).await;
                }
                Err(e) => {
                    health::report(Fault::Backend, e);
                    failures += 1;
                    if failures > MAX_FAILURES {
                        return;
                    }
                    Timer::after_secs(RETRY_DELAY_S).await;
                }
            }
        }
    }
}

/// Never returns with missing credentials
async fn load_credentials() -> WifiCredentials {
    match WifiCredentials::load() {
        Ok(creds) => creds,
        Err(err) => {
            log::error!("Flash WiFi credentials with scripts/flash_blobs.py");
            health::report(Fault::WifiCredentials, err);
            core::future::pending().await
        }
    }
}

pub fn spawn(
    spawner: Spawner,
    control: Control<'static>,
    stack: Stack<'static>,
    http_client: &'static HttpClient,
) {
    let actor = WifiActor {
        control,
        stack,
        http_client,
    };
    spawner.spawn(wifi_task(actor).unwrap());
}

#[embassy_executor::task]
async fn wifi_task(mut actor: WifiActor) -> ! {
    actor.run().await
}
