use embassy_executor::Spawner;
use embassy_net::{Config, DhcpConfig, Stack, StackResources};
use embassy_rp::clocks::RoscRng;
use embassy_rp::gpio::{Level, Output};
use embassy_rp::peripherals::{DMA_CH0, PIN_23, PIN_24, PIN_25, PIN_29, PIO0};
use embassy_rp::pio::Pio;
use embassy_rp::{Peri, dma};
use static_cell::StaticCell;

use cyw43::{A4, Aligned, Control, NetDriver, Runner, SpiBus};
use cyw43_pio::{DEFAULT_CLOCK_DIVIDER, PioSpi};

use super::http_client::HttpClient;
use super::wifi;
use crate::config::flash_config::{CLM_ADDR, CLM_SIZE, FW_ADDR, FW_SIZE, NVRAM_ADDR, NVRAM_SIZE};
use crate::interrupts::Irqs;

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
static HTTP_CLIENT: StaticCell<HttpClient> = StaticCell::new();

/// Bring up the CYW43 chip and the embassy-net stack, spawning their runners
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

/// Connect to WiFi, wait for an IP and spawn the network tasks
pub async fn spawn(
    spawner: Spawner,
    pinning: NetworkPinning,
) -> (&'static HttpClient, &'static Stack<'static>) {
    let seed = RoscRng.next_u64();
    let (stack, control) = setup_stack(spawner, pinning, seed).await;

    let http_client = HTTP_CLIENT.init(HttpClient::new(stack));

    wifi::spawn(spawner, control, *stack, http_client);

    log::info!("Network stack initialized");

    // Wait for network to be ready
    log::info!("Waiting for network link...");
    stack.wait_link_up().await;
    log::info!("Network link is up, waiting for ip...");

    stack.wait_config_up().await;
    match stack.config_v4() {
        Some(config) => log::info!("Network configured with IP: {}", config.address.address()),
        None => log::info!("Network configured without IPv4"),
    }

    (http_client, stack)
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
