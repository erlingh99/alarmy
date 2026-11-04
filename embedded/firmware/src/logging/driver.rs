use core::fmt::Write;
use embassy_executor::Spawner;
use embassy_futures::join::join;
use embassy_futures::select::{select, Either};
use embassy_rp::peripherals::USB;
use embassy_rp::usb::Driver;
use embassy_rp::Peri;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::{Channel, Receiver};
use embassy_sync::signal::Signal;
use embassy_usb::class::cdc_acm::{CdcAcmClass, State};
use embassy_usb::{Builder, Config};
use heapless::String;
use log::{Level, Log, Metadata, Record};

use super::TruncatingWriter;

const LINE_LEN: usize = 256;
const LOG_LINE_CAPACITY: usize = 8;
const MAX_PACKET_SIZE: u8 = 64;

/// The oldest line is dropped when full
static HTTP_LOG_CHANNEL: Channel<CriticalSectionRawMutex, String<LINE_LEN>, LOG_LINE_CAPACITY> =
    Channel::new();
/// Held until a terminal sets DTR, the oldest lines are dropped when full
static USB_LOG_CHANNEL: Channel<CriticalSectionRawMutex, String<LINE_LEN>, LOG_LINE_CAPACITY> =
    Channel::new();
static MULTI_LOGGER: MultiLogger = MultiLogger;
/// Signalled by logger_task once set_logger_racy has been called, so initialize_logger
/// can return only after logging is ready
static LOGGER_READY: Signal<CriticalSectionRawMutex, ()> = Signal::new(); //NoopRawMutex should be sufficient here

use crate::interrupts::Irqs;

pub struct LoggingPinning {
    pub pin: Peri<'static, USB>,
}

fn format_record<'a>(record: &Record, buf: &'a mut [u8; LINE_LEN]) -> &'a str {
    let level = record.level();
    let color = match level {
        Level::Error => "\x1b[0;31m",
        Level::Warn => "\x1b[0;33m",
        Level::Info => "\x1b[0;37m",
        Level::Debug => "\x1b[0;36m",
        Level::Trace => "\x1b[0;32m",
    };
    let reset = "\x1b[0m";
    let mut writer = TruncatingWriter::new(buf, Some("..."), Some("\r\n"));
    let _ = write!(writer, "{color}[{level}]{reset} {}", record.args());
    writer.finish()
}

fn push_line(
    channel: &Channel<CriticalSectionRawMutex, String<LINE_LEN>, LOG_LINE_CAPACITY>,
    line: String<LINE_LEN>,
) {
    critical_section::with(|_| {
        if channel.is_full() {
            let _ = channel.try_receive();
        }
        let _ = channel.try_send(line);
    });
}

pub struct MultiLogger;

impl MultiLogger {
    /// Spawn on a thread mode executor: its wakes (`sev`) reach both cores, so core1 can log
    pub async fn initialize(
        spawner: Spawner,
        pinning: LoggingPinning,
    ) -> Receiver<'static, CriticalSectionRawMutex, String<LINE_LEN>, 8> {
        spawner.spawn(logger_task(pinning, &LOGGER_READY).unwrap());
        // Wait until logger_task has registered the logger via set_logger_racy before returning,
        // so callers can safely use log! macros immediately after this function resolves.
        LOGGER_READY.wait().await;
        HTTP_LOG_CHANNEL.receiver()
    }
}

impl Log for MultiLogger {
    fn enabled(&self, _: &Metadata) -> bool {
        true
    }

    fn log(&self, record: &Record) {
        let mut buf = [0; LINE_LEN];
        let Ok(line) = String::try_from(format_record(record, &mut buf)) else {
            return;
        };
        push_line(&USB_LOG_CHANNEL, line.clone());
        push_line(&HTTP_LOG_CHANNEL, line);
    }

    fn flush(&self) {}
}

#[embassy_executor::task]
async fn logger_task(
    pinning: LoggingPinning,
    ready_signal: &'static Signal<CriticalSectionRawMutex, ()>,
) -> ! {
    // Create the driver, from the HAL.
    let driver: Driver<'static, USB> = Driver::new(pinning.pin, Irqs);
    // Create embassy-usb Config
    let mut config = Config::new(0xc0de, 0xcafe);
    config.manufacturer = Some("Embassy");
    config.product = Some("USB-logging");
    config.serial_number = Some("12345678");
    config.max_power = 100;
    config.max_packet_size_0 = MAX_PACKET_SIZE;

    // Create embassy-usb DeviceBuilder using the driver and config.
    // It needs some buffers for building the descriptors.
    let mut config_descriptor = [0; 256];
    let mut bos_descriptor = [0; 256];
    let mut control_buf = [0; 64];

    let mut logger_state = State::new();

    let mut builder = Builder::new(
        driver,
        config,
        &mut config_descriptor,
        &mut bos_descriptor,
        &mut [], // no msos descriptors
        &mut control_buf,
    );

    // Create a class for the logger
    let logger_class = CdcAcmClass::new(&mut builder, &mut logger_state, MAX_PACKET_SIZE as u16);
    let (mut sender, _, control) = logger_class.split_with_control();
    // Build the builder.
    let mut usb_device = builder.build();

    let log_fut = async {
        // Kept until fully sent, so a disconnect mid-line resumes where it left off
        let mut line = String::<LINE_LEN>::new();
        let mut sent = 0;
        loop {
            sender.wait_connection().await;
            // Windows drops data sent before DTR
            if !control.dtr() {
                control.control_changed().await;
                continue;
            }
            if sent == line.len() {
                line = match select(USB_LOG_CHANNEL.receive(), control.control_changed()).await {
                    Either::First(line) => line,
                    Either::Second(()) => continue,
                };
                sent = 0;
            }
            let n = (line.len() - sent).min(MAX_PACKET_SIZE as usize);
            if sender
                .write_packet(&line.as_bytes()[sent..sent + n])
                .await
                .is_err()
            {
                continue;
            }
            sent += n;
            // A full packet that ends the data needs a zero-length packet to end the transfer
            if n == MAX_PACKET_SIZE as usize && sent == line.len() && USB_LOG_CHANNEL.is_empty() {
                let _ = sender.write_packet(&[]).await;
            }
        }
    };

    // SAFETY: set_logger_racy is used because we are in a single-threaded async context
    // during initialisation. ready_signal is signalled immediately after, so no log! call
    // can race with this registration.
    unsafe {
        let _ = log::set_logger_racy(&MULTI_LOGGER)
            .map(|()| log::set_max_level_racy(log::LevelFilter::Info));
    }

    // Run the USB device.
    let usb_fut = usb_device.run();

    ready_signal.signal(());
    // Run everything concurrently.
    join(usb_fut, log_fut).await;
    panic!();
}
