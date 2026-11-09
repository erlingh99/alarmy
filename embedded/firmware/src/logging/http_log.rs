use embassy_executor::Spawner;
use embassy_time::Timer;
use heapless::{String, Vec};

use super::logger::{HTTP_LOG_CHANNEL, LINE_LEN, LOG_LINE_CAPACITY};
use crate::net::HttpClient;

const JSON_CAPACITY: usize = LINE_LEN * (LOG_LINE_CAPACITY + 2); // +2 for the json syntax overhead

/// Posts the log lines to the backend in batches
struct HttpLogUploader {
    http_client: &'static HttpClient,
}

impl HttpLogUploader {
    async fn run(&self) -> ! {
        loop {
            let mut batch: Vec<String<LINE_LEN>, LOG_LINE_CAPACITY> = Vec::new();
            let _ = batch.push(HTTP_LOG_CHANNEL.receive().await);

            while !batch.is_full() {
                Timer::after_millis(100).await;
                match HTTP_LOG_CHANNEL.try_receive() {
                    Ok(line) => {
                        let _ = batch.push(line);
                    }
                    Err(_) => break,
                }
            }

            self.post(&batch).await;
        }
    }

    /// Posts the lines, halving any part whose JSON escaping overflows the buffer
    async fn post(&self, lines: &[String<LINE_LEN>]) {
        let mut parts: Vec<&[String<LINE_LEN>], LOG_LINE_CAPACITY> = Vec::new();
        let _ = parts.push(lines);
        while let Some(part) = parts.pop() {
            match serde_json_core::to_vec::<_, JSON_CAPACITY>(part) {
                Ok(json) => {
                    let _ = self.http_client.post_log(&json).await;
                }
                // A single line always fits, even fully escaped
                Err(_) if part.len() > 1 => {
                    let (first, second) = part.split_at(part.len() / 2);
                    // Popped last first, so the lines go out in order
                    let _ = parts.push(second);
                    let _ = parts.push(first);
                }
                Err(_) => {}
            }
        }
    }
}

/// Spawn once the network is up. Lines logged before then are kept up to the channel's capacity.
pub fn spawn(spawner: Spawner, http_client: &'static HttpClient) {
    spawner.spawn(http_log_task(HttpLogUploader { http_client }).unwrap());
}

#[embassy_executor::task]
async fn http_log_task(uploader: HttpLogUploader) -> ! {
    uploader.run().await
}
