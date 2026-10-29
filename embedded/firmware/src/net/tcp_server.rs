use core::fmt::Write;
use embassy_net::tcp::TcpSocket;
use embassy_net::Stack;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::{Channel, Receiver};
use embassy_time::{Duration, Timer};
use heapless::String;
use serde::Deserialize;
use smart_leds::RGB8;

use super::http_client::NetworkError;
use crate::led::deserialize::deserialize_hex;

// TCP server configuration
pub const TCP_SERVER_PORT: u16 = 8080;
pub const COMMAND_BUFFER_SIZE: usize = 512;

static COMMAND_CHANNEL: Channel<CriticalSectionRawMutex, TcpCommand, 1> = Channel::new();

pub struct TcpServer {
    stack: &'static Stack<'static>,
    command_channel: &'static Channel<CriticalSectionRawMutex, TcpCommand, 1>,
}

#[derive(Deserialize, Debug)]
pub enum TcpCommand {
    AlarmsUpdated,
    Idle,
    Fire,
    Rainbow,
    #[serde(deserialize_with = "deserialize_hex")]
    Color(RGB8),
    CheckUpdate,
    Version,
}

impl TcpServer {
    pub fn new(stack: &'static Stack<'static>) -> Self {
        Self {
            stack,
            command_channel: &COMMAND_CHANNEL,
        }
    }

    pub fn subscribe(&self) -> Receiver<'static, CriticalSectionRawMutex, TcpCommand, 1> {
        self.command_channel.receiver()
    }

    pub async fn run(&self) -> ! {
        log::info!("Starting TCP server on port {TCP_SERVER_PORT}");

        let mut rx_buffer = [0; COMMAND_BUFFER_SIZE];
        let mut tx_buffer = [0; COMMAND_BUFFER_SIZE];

        loop {
            let mut socket = TcpSocket::new(*self.stack, &mut rx_buffer, &mut tx_buffer);
            socket.set_timeout(Some(Duration::from_secs(30)));

            if let Err(e) = socket.accept(TCP_SERVER_PORT).await {
                log::error!("TCP accept error: {e:?}");
                Timer::after_secs(1).await;
                continue;
            }

            log::debug!("TCP client connected");

            // Handle the connection
            if let Err(e) = self.handle_connection(&mut socket).await {
                log::error!("TCP connection error: {e:?}");
            }

            log::debug!("TCP client disconnected");
        }
    }

    async fn handle_connection(&self, socket: &mut TcpSocket<'_>) -> Result<(), NetworkError> {
        let mut buffer = [0u8; COMMAND_BUFFER_SIZE];

        // Send welcome message
        let mut welcome = String::<40>::new();
        let _ = writeln!(welcome, "Connected to Pico TCP Server");
        let _ = socket.write(welcome.as_bytes()).await;

        loop {
            // Read command from client
            let n = match socket.read(&mut buffer).await {
                Ok(0) => {
                    // Connection closed by client
                    break;
                }
                Ok(n) => n,
                Err(_) => {
                    return Err(NetworkError::TcpError);
                }
            };

            let command = match serde_json_core::from_slice::<TcpCommand>(&buffer[..n]) {
                Ok((command, _)) => command,
                Err(_) => {
                    let response = "ERROR: Invalid command\n";
                    let _ = socket.write(response.as_bytes()).await;
                    continue;
                }
            };

            log::debug!("Received TCP command: {command:?}");

            if matches!(command, TcpCommand::Version) {
                let mut response = String::<32>::new();
                let _ = writeln!(response, "VERSION: {}", env!("CARGO_PKG_VERSION"));
                let _ = socket.write(response.as_bytes()).await;
                continue;
            }

            if self.command_channel.try_send(command).is_err() {
                let response = "ERROR: Command queue full\n";
                let _ = socket.write(response.as_bytes()).await;
                continue;
            }

            let response = "OK: Command queued\n";
            if socket.write(response.as_bytes()).await.is_err() {
                return Err(NetworkError::TcpError);
            }
        }

        Ok(())
    }
}

#[embassy_executor::task]
pub async fn tcp_server_task(server: &'static TcpServer) -> ! {
    server.run().await
}
