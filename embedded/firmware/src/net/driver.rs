use embassy_executor::Spawner;
use embassy_net::{Config, DhcpConfig, Stack, StackResources};
use embassy_rp::clocks::RoscRng;
use embassy_rp::gpio::{Level, Output};
use embassy_rp::peripherals::{DMA_CH0, PIN_23, PIN_24, PIN_25, PIN_29, PIO0};
use embassy_rp::pio::Pio;
use embassy_rp::{dma, Peri};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Receiver;
use embassy_time::Timer;
use heapless::{String, Vec};
use portable_atomic::{AtomicBool, Ordering};
use static_cell::StaticCell;

use cyw43::{Aligned, Control, JoinAuth, JoinOptions, NetDriver, Runner, SpiBus, A4};
use cyw43_pio::{PioSpi, DEFAULT_CLOCK_DIVIDER};

use super::http_client::HttpClientManager;
use super::tcp_server::TcpServer;
use crate::config::api_config::RETRY_DELAY_S;
use crate::config::flash_config::{CLM_ADDR, CLM_SIZE, FW_ADDR, FW_SIZE, NVRAM_ADDR, NVRAM_SIZE};
use crate::config::wifi_config::WifiCredentials;
use crate::interrupts::Irqs;
use crate::net::tcp_server::tcp_server_task;

pub struct NetworkPinning {
    pub pio_pin: Peri<'static, PIO0>,
    pub pwr_pin: Peri<'static, PIN_23>,
    pub cs_pin: Peri<'static, PIN_25>,
    pub dio_pin: Peri<'static, PIN_24>,
    pub clk_pin: Peri<'static, PIN_29>,
    pub dma0: Peri<'static, DMA_CH0>,
}

static STATE: StaticCell<cyw43::State> = StaticCell::new();
static RESOURCES: StaticCell<StackResources<6>> = StaticCell::new();
static STACK: StaticCell<Stack> = StaticCell::new();
static HTTP_CLIENT: StaticCell<HttpClientManager> = StaticCell::new();
static TCP_SERVER: StaticCell<TcpServer> = StaticCell::new();

pub static WIFI_CONNECTED: AtomicBool = AtomicBool::new(false);

pub struct NetworkManager;

impl NetworkManager {
    /// Initialize the network stack and start background tasks
    async fn setup_stack(
        spawner: Spawner,
        pinning: NetworkPinning,
        seed: u64,
    ) -> (&'static mut Stack<'static>, Control<'static>) {
        log::info!("Initializing network stack...");

        // Initialize PIO for SPI communication with CYW43
        let pwr = Output::new(pinning.pwr_pin, Level::Low);
        let cs = Output::new(pinning.cs_pin, Level::High);
        let mut pio = Pio::new(pinning.pio_pin, Irqs);
        let spi = PioSpi::new(
            &mut pio.common,
            pio.sm0,
            DEFAULT_CLOCK_DIVIDER,
            pio.irq0,
            cs,
            pinning.dio_pin,
            pinning.clk_pin,
            dma::Channel::new(pinning.dma0, Irqs),
        );

        // CYW43 firmware cyw43 firmware is large, preflash it at known adress and read from those
        let fw_blob = unsafe { core::slice::from_raw_parts(FW_ADDR, FW_SIZE) };
        let clm_blob = unsafe { core::slice::from_raw_parts(CLM_ADDR, CLM_SIZE) };
        let nvram_blob = unsafe { core::slice::from_raw_parts(NVRAM_ADDR, NVRAM_SIZE) };
        let fw = unsafe { &*(fw_blob as *const [u8] as *const Aligned<A4, [u8]>) };
        let clm = unsafe { &*(clm_blob as *const [u8] as *const Aligned<A4, [u8]>) };
        let nvram = unsafe { &*(nvram_blob as *const [u8] as *const Aligned<A4, [u8]>) };

        // Initialize CYW43 driver
        let state = STATE.init(cyw43::State::new());
        let (net_device, mut control, runner) = cyw43::new(state, pwr, spi, fw, nvram).await;

        // Start CYW43 background task
        spawner.spawn(cyw43_task(runner).unwrap());

        // Init control
        control.init(clm).await;
        control
            .set_power_management(cyw43::PowerManagementMode::PowerSave)
            .await;

        let mut dhcp_config = DhcpConfig::default();
        dhcp_config.hostname = Some(heapless::String::try_from("alarmy-pico").unwrap());
        let config = Config::dhcpv4(dhcp_config);

        let resources = RESOURCES.init(StackResources::<6>::new());
        let (stack, runner) = embassy_net::new(net_device, config, resources, seed);
        // Start network task
        spawner.spawn(net_task(runner).unwrap());

        let stack_ref = STACK.init(stack);

        (stack_ref, control)
    }

    /// Create a new NetworkManager instance
    pub async fn initialize(
        spawner: Spawner,
        pinning: NetworkPinning,
        log_queue: Receiver<'static, CriticalSectionRawMutex, String<256>, 8>,
    ) -> (&'static HttpClientManager, &'static TcpServer) {
        let seed = RoscRng.next_u64();
        let (stack, control) = Self::setup_stack(spawner, pinning, seed).await;

        let http_client = HTTP_CLIENT.init(HttpClientManager::new(stack));
        let tcp_server = TCP_SERVER.init(TcpServer::new(stack));

        // Connect to WiFi
        spawner.spawn(wifi_connect_task(control, http_client).unwrap());

        log::info!("Network stack initialized");

        // Wait for network to be ready
        log::info!("Waiting for network link...");
        stack.wait_link_up().await;
        log::info!("Network link is up, waiting for ip...");

        stack.wait_config_up().await;
        if let Some(config) = stack.config_v4() {
            log::info!("Network configured with IP: {}", config.address.address());
        } else {
            log::info!("Network configured without IPv4");
        }

        // Start background tasks
        spawner.spawn(log_over_http(log_queue, http_client).unwrap());

        Timer::after_secs(1).await;
        spawner.spawn(tcp_server_task(tcp_server).unwrap());

        (http_client, tcp_server)
    }
}

const MAX_BACKOFF_SHIFT: u32 = 6;

#[embassy_executor::task]
pub async fn wifi_connect_task(
    mut control: cyw43::Control<'static>,
    client: &'static HttpClientManager,
) -> ! {
    let creds = loop {
        match WifiCredentials::load() {
            Ok(creds) => break creds,
            Err(err) => {
                // Nothing to do until the device is reflashed with credentials
                log::error!("No usable WiFi credentials in flash ({err:?}), flash them with scripts/flash_blobs.py");
                Timer::after_secs(60).await;
            }
        }
    };
    let join_options = if creds.psk.is_empty() {
        JoinOptions::new_open()
    } else {
        // A prehashed PSK only works for WPA2 (WPA3/SAE needs the passphrase)
        let mut options = JoinOptions::new(&creds.psk);
        options.auth = JoinAuth::Wpa2;
        options.passphrase_is_prehashed = true;
        options
    };

    let mut connection_attempts = 0;
    loop {
        match control.join(&creds.ssid, join_options.clone()).await {
            Ok(_) => {
                log::info!("WiFi connected successfully");
                connection_attempts = 0;
                WIFI_CONNECTED.store(true, Ordering::Relaxed);
                // Keep the connection alive and monitor status
                let mut api_attempts = 0;

                Timer::after_secs(300).await;
                loop {
                    if let Err(e) = client.get_health().await {
                        log::info!("Health check failed {e:#?}");
                        api_attempts += 1;
                        if api_attempts > 10 {
                            break;
                        }
                        Timer::after_secs(RETRY_DELAY_S).await;
                    } else {
                        api_attempts = 0;
                        Timer::after_secs(3600).await;
                    }
                }
                WIFI_CONNECTED.store(false, Ordering::Relaxed);
                control.leave().await;
                log::error!("Internet or backend down");
            }
            Err(err) => {
                WIFI_CONNECTED.store(false, Ordering::Relaxed);
                log::error!("WiFi connection failed. Status: {err:#?}");
            }
        }
        Timer::after_secs(RETRY_DELAY_S << connection_attempts).await; // exponential backoff
        log::info!("Retrying WiFi connection...");
        // Cap the backoff at RETRY_DELAY_S << MAX_BACKOFF_SHIFT (5 s * 64 = ~5 min)
        connection_attempts = (connection_attempts + 1).min(MAX_BACKOFF_SHIFT);
    }
}

#[embassy_executor::task]
pub async fn cyw43_task(
    runner: Runner<'static, SpiBus<Output<'static>, PioSpi<'static, PIO0, 0>>>,
) -> ! {
    runner.run().await
}

#[embassy_executor::task]
pub async fn net_task(mut runner: embassy_net::Runner<'static, NetDriver<'static>>) -> ! {
    runner.run().await
}

#[embassy_executor::task]
pub async fn log_over_http(
    log_queue: Receiver<'static, CriticalSectionRawMutex, String<256>, 8>,
    http_client: &'static HttpClientManager,
) -> ! {
    loop {
        let mut batch: Vec<String<256>, 8> = Vec::new();
        let _ = batch.push(log_queue.receive().await);

        while !batch.is_full() {
            Timer::after_millis(100).await;
            match log_queue.try_receive() {
                Ok(msg) => {
                    let _ = batch.push(msg);
                }
                Err(_) => break,
            }
        }

        if let Ok(json) = serde_json_core::to_vec::<_, 2304>(&batch) {
            let _ = http_client.post_log(&json).await;
        }
    }
}
