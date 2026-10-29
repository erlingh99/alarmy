use core::fmt::Write;
use embassy_executor::Spawner;
use embassy_futures::join::join;
use embassy_rp::peripherals::USB;
use embassy_rp::usb::Driver;
use embassy_rp::Peri;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::{Channel, Receiver, Sender};
use embassy_sync::signal::Signal;
use embassy_usb::class::cdc_acm::{CdcAcmClass, State};
use embassy_usb::driver::Driver as UsbDriver;
use embassy_usb::{Builder, Config};
use embassy_usb_logger::{DummyHandler, UsbLogger};
use heapless::String;
use log::{Level, Log, Metadata, Record};
use static_cell::StaticCell;

static HTTP_LOG_CHANNEL: Channel<CriticalSectionRawMutex, String<256>, 8> = Channel::new();
static MULTI_LOGGER: StaticCell<MultiLogger> = StaticCell::new();
/// Signalled by logger_task once set_logger_racy has been called, so initialize_logger
/// can return only after logging is ready
static LOGGER_READY: Signal<CriticalSectionRawMutex, ()> = Signal::new(); //NoopRawMutex should be sufficient here

use crate::interrupts::Irqs;

pub struct LoggingPinning {
    pub pin: Peri<'static, USB>,
}

fn format_record<const N: usize>(record: &Record) -> String<N> {
    let level = record.level();
    let color = match level {
        Level::Error => "\x1b[0;31m",
        Level::Warn => "\x1b[0;33m",
        Level::Info => "\x1b[0;37m",
        Level::Debug => "\x1b[0;36m",
        Level::Trace => "\x1b[0;32m",
    };
    let reset = "\x1b[0m";
    let mut s = String::new();
    write!(&mut s, "{color}[{level}]{reset} {}\r\n", record.args()).unwrap_or_default();
    s
}

struct HttpLogger<const N: usize, const M: usize> {
    http_log_queue: Sender<'static, CriticalSectionRawMutex, String<N>, M>,
}

impl<const N: usize, const M: usize> Log for HttpLogger<N, M> {
    fn enabled(&self, _: &Metadata) -> bool {
        true
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let _ = self.http_log_queue.try_send(format_record(record));
    }

    fn flush(&self) {}
}

pub struct MultiLogger {
    usb_logger: UsbLogger<512, DummyHandler>,
    http_logger: HttpLogger<256, 8>,
}

impl MultiLogger {
    pub async fn initialize(
        spawner: Spawner,
        pinning: LoggingPinning,
    ) -> Receiver<'static, CriticalSectionRawMutex, String<256>, 8> {
        spawner.spawn(logger_task(pinning, HTTP_LOG_CHANNEL.sender(), &LOGGER_READY).unwrap());
        // Wait until logger_task has registered the logger via set_logger_racy before returning,
        // so callers can safely use log! macros immediately after this function resolves.
        LOGGER_READY.wait().await;
        HTTP_LOG_CHANNEL.receiver()
    }

    pub async fn create_usb_future_from_class<'d, D>(&'d self, class: CdcAcmClass<'d, D>)
    where
        D: UsbDriver<'d>,
    {
        self.usb_logger.create_future_from_class(class).await
    }
}

impl Log for MultiLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        self.usb_logger.enabled(metadata) || self.http_logger.enabled(metadata)
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            self.usb_logger.log(record);
            self.http_logger.log(record);
        }
    }

    fn flush(&self) {
        self.usb_logger.flush();
        self.http_logger.flush();
    }
}

#[embassy_executor::task]
async fn logger_task(
    pinning: LoggingPinning,
    http_log_queue: Sender<'static, CriticalSectionRawMutex, String<256>, 8>,
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
    config.max_packet_size_0 = 64;

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
    let logger_class = CdcAcmClass::new(&mut builder, &mut logger_state, 64);
    // Build the builder.
    let mut usb_device = builder.build();

    let usb_logger = UsbLogger::with_custom_style(|record, writer| {
        let msg = format_record::<256>(record);
        writer.write_str(msg.as_str()).unwrap();
    });

    let http_logger = HttpLogger { http_log_queue };

    let logger_ref = MULTI_LOGGER.init(MultiLogger {
        usb_logger,
        http_logger,
    });

    let log_fut = logger_ref.create_usb_future_from_class(logger_class);

    // SAFETY: set_logger_racy is used because we are in a single-threaded async context
    // during initialisation. ready_signal is signalled immediately after, so no log! call
    // can race with this registration.
    unsafe {
        let _ = log::set_logger_racy(logger_ref)
            .map(|()| log::set_max_level_racy(log::LevelFilter::Debug));
    }

    // Run the USB device.
    let usb_fut = usb_device.run();

    ready_signal.signal(());
    // Run everything concurrently.
    join(usb_fut, log_fut).await;
    panic!();
}
