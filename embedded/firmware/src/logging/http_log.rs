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

            if let Ok(json) = serde_json_core::to_vec::<_, JSON_CAPACITY>(&batch) {
                let _ = self.http_client.post_log(&json).await;
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
