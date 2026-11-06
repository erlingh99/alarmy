use core::fmt::Write;
use embassy_executor::Spawner;
use embassy_net::Stack;
use embassy_net::tcp::TcpSocket;
use embassy_time::{Duration, Timer};
use heapless::String;
use serde::Deserialize;
use smart_leds::RGB8;

use super::http_client::NetworkError;
use crate::input::{InputHandle, RemoteCommand};
use crate::led::deserialize::deserialize_hex;

// TCP server configuration
pub const TCP_SERVER_PORT: u16 = 8080;
pub const COMMAND_BUFFER_SIZE: usize = 512;

#[derive(Deserialize, Debug)]
enum TcpCommand {
    AlarmsUpdated,
    CheckUpdate,
    Idle,
    Fire,
    Rainbow,
    #[serde(deserialize_with = "deserialize_hex")]
    Color(RGB8),
    Version,
}

/// Turns backend commands into input events
/// Answers `Version` itself
struct TcpServer {
    stack: &'static Stack<'static>,
    input: InputHandle,
}

impl TcpServer {
    async fn run(&self) -> ! {
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
        let _ = writeln!(welcome, "Connected to Alarmy Pico TCP Server");
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

            let response = self.handle_command(command);
            if socket.write(response.as_bytes()).await.is_err() {
                return Err(NetworkError::TcpError);
            }
        }

        Ok(())
    }

    fn handle_command(&self, command: TcpCommand) -> String<32> {
        let mut response = String::new();
        let remote = match command {
            TcpCommand::AlarmsUpdated => RemoteCommand::AlarmsUpdated,
            TcpCommand::CheckUpdate => RemoteCommand::CheckUpdate,
            TcpCommand::Idle => RemoteCommand::Idle,
            TcpCommand::Fire => RemoteCommand::Fire,
            TcpCommand::Rainbow => RemoteCommand::Rainbow,
            TcpCommand::Color(c) => RemoteCommand::Color(c),
            TcpCommand::Version => {
                let _ = writeln!(response, "VERSION: {}", env!("CARGO_PKG_VERSION"));
                return response;
            }
        };
        let _ = match self.input.remote_command(remote) {
            Ok(()) => writeln!(response, "OK: Command queued"),
            Err(_) => writeln!(response, "ERROR: Command queue full"),
        };
        response
    }
}

pub fn spawn(spawner: Spawner, stack: &'static Stack<'static>, input: InputHandle) {
    let server = TcpServer { stack, input };
    spawner.spawn(tcp_server_task(server).unwrap());
}

#[embassy_executor::task]
async fn tcp_server_task(server: TcpServer) -> ! {
    server.run().await
}
