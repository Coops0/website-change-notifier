use std::fmt::{Debug, Formatter};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use pushover_rs::{MessageBuilder, send_pushover_request};
use tokio::time;

struct WebsiteData {
    url: String,
    last_response: Option<String>,
    merch_already_detected: bool,

    // in a row, count the number of times i have been texted, used for cooldown
    changes_stacking: u8,
    // if notified consecutively >= 4 times, add a cooldown that increases more with each cooldown
    current_cooldown: u16,
    // counts the number of cooldowns recieved, decreases one per successful blank/cycle
    total_cooldowns: u32,
}

impl Debug for WebsiteData {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f
            .debug_struct("WebsiteData")
            .field("url", &self.url)
            .field("last_response", &self.last_response.as_ref().map(|r| r.len()))
            .field("merch_already_detected", &self.merch_already_detected)
            .field("changes_stacking", &self.changes_stacking)
            .field("cooldown", &self.current_cooldown)
            .field("cooldowns", &self.total_cooldowns)
            .finish()
    }
}

impl WebsiteData {
    // should run init request, basically check if its on a cooldown
    fn should_request(&mut self) -> bool {
        if self.current_cooldown == 0 {
            return true;
        }

        self.current_cooldown -= 1;
        self.current_cooldown == 0
    }

    // INFERS CHANGES ARE DETECTED, if they are then calculate if this notification should result in a cooldown instead
    fn should_notify(&mut self) -> bool {
        self.changes_stacking += 1;
        let banned = self.changes_stacking >= 4;

        if banned {
            self.total_cooldowns += 1;
            self.current_cooldown = (3_u16).pow(self.total_cooldowns);
            self.changes_stacking = 0;

            println!("Cooldown given for {} for {} cycles, stacked cooldowns={}", self.url, self.current_cooldown, self.total_cooldowns);
        }

        !banned
    }
}

impl From<&str> for WebsiteData {
    fn from(value: &str) -> Self {
        WebsiteData {
            url: value.to_string(),
            last_response: None,
            merch_already_detected: false,

            changes_stacking: 0,
            current_cooldown: 0,
            total_cooldowns: 0,
        }
    }
}


#[tokio::main]
async fn main() {
    let reqwest_client = reqwest::Client::default();

    let mut interval = time::interval(Duration::from_secs(10));
    let mut sites: Vec<WebsiteData> = vec![
        "https://www.kevinabstract.co".into(),
        "https://luckyedwards.com".into(),
        "https://videostore.world/".into(),
        "https://shop.holidaybrand.co/".into(),
    ];

    let merch_keywords = [
        "merch",
        "store",
        "shop",
        "stock",
        "buy",
        "cloth",
        "shirt",
        "hood",
        "tee"
    ];

    loop {
        interval.tick().await;

        for site in &mut sites {
            println!("{site:?}");
            if !site.should_request() {
                continue;
            }

            let request = reqwest_client
                .get(&site.url)
                .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/117.0.0.0 Safari/537.36")
                .build()
                .expect("Failed to build request");

            let response = match reqwest_client.execute(request).await {
                Ok(o) => o,
                Err(e) => {
                    eprint!("Error requesting {} -> {:?}", site.url, e);
                    continue;
                }
            };

            let text = response.text()
                .await
                .expect("Failed to receive text from site");

            if Some(&text) == site.last_response.as_ref() {
                if site.total_cooldowns != 0 {
                    site.total_cooldowns -= 1;
                }

                site.changes_stacking = 0;

                continue;
            }

            let first_run = site.last_response.is_none();
            let mut merch_newly_detected = merch_keywords.iter()
                .any(|k| text.to_lowercase().contains(k));

            if site.merch_already_detected {
                merch_newly_detected = false;
            } else {
                site.merch_already_detected = merch_newly_detected;
            }

            site.last_response = Some(text);
            if first_run {
                continue;
            }

            if site.should_notify() {
                notify(&site, if merch_newly_detected { 1 } else { 0 }).await;
            }
        }
    }
}

async fn notify(website: &WebsiteData, priority: i8) {
    println!("Notifying for {}...", website.url);

    let duration_since_epoch = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    let now = duration_since_epoch.as_secs();

    let message = MessageBuilder::new("ucj95tt4g9nwx5zjacepr2brsyi3ot", "a4sijp87u6n8sr7otzvg3r6jbdm4h6", &website.url)
        .set_title("Website Change Detected")
        .set_url(&website.url, None)
        .add_device("iphone")
        .set_timestamp(now)
        .set_priority(priority)
        .build();

    if let Err(e) = send_pushover_request(message).await {
        eprint!("Error sending message {e:?}");
    }
}