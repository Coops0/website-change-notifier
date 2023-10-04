use std::fmt::{Debug, Formatter};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chromiumoxide::{Browser, Page};
use chromiumoxide::browser::BrowserConfigBuilder;
use chromiumoxide::page::ScreenshotParams;
use futures::StreamExt;
use image::RgbImage;
use pushover_rs::{MessageBuilder, send_pushover_request};
use tokio::{task, time};
use tokio::time::sleep;

const MERCH_KEYWORDS: &[&str] = &[
    "merch",
    "store",
    "shop",
    "buy",
    "clothing",
    "clothes",
];

struct WebsiteData {
    url: String,
    last_image: Option<RgbImage>,
    merch_already_detected: bool,

    // in a row, count the number of times i have been texted, used for cooldown
    changes_stacking: u8,
    // if notified consecutively >= 4 times, add a cooldown that increases more with each cooldown
    current_cooldown: u16,
    // counts the number of cooldowns recieved, decreases one per successful blank/cycle
    total_cooldowns: u32,

    total_runs: u32,
}

impl Debug for WebsiteData {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f
            .debug_struct("WebsiteData")
            .field("url", &self.url)
            .field("last_response", &self.last_image.as_ref().map(|r| r.len()))
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
            last_image: None,
            merch_already_detected: false,
            total_runs: 0,

            changes_stacking: 0,
            current_cooldown: 0,
            total_cooldowns: 0,
        }
    }
}


#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut sites: Vec<WebsiteData> = vec![
        "https://www.kevinabstract.co".into(),
        "https://luckyedwards.com".into(),
        "https://videostore.world/".into(),
        "https://shop.holidaybrand.co/".into(),
    ];

    let (browser, mut handler) = Browser::launch(
        BrowserConfigBuilder::default()
            .request_timeout(Duration::from_secs(5))
            .build()
            .unwrap()
    ).await?;

    let _ = task::spawn(async move {
        while let Some(h) = handler.next().await {
            if let Err(e) = h {
                eprintln!("handler error -> {e:?}");
                break;
            }
        }
    });

    let page = browser.new_page("about:blank").await?;
    page.set_user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/116.0.0.0 Safari/537.36").await?;

    let mut interval = time::interval(Duration::from_secs(25));
    loop {
        interval.tick().await;

        for site in &mut sites {
            if let Err(e) = check_site(&page, site).await {
                eprintln!("Error checking site {} -> {e:?}", site.url);
            }
        }

        page.goto("about:blank").await?;
    }
}

async fn check_site(page: &Page, site: &mut WebsiteData) -> anyhow::Result<()> {
    println!("{site:?}");
    if !site.should_request() {
        return Ok(());
    }

    site.total_runs += 1;

    let first_run = site.last_image.is_none();
    let last_image = site.last_image.take();

    let mut screenshot_scores = vec![];
    for _ in 0..3 {
        let result = create_screenshot(page, site, &last_image).await?;
        if result.0 > 0.98 {
            screenshot_scores.push(result);
            break;
        }

        screenshot_scores.push(result);
        sleep(Duration::from_millis(250)).await;
    }

    let only_scores = screenshot_scores.iter().map(|(s, _)| *s).collect::<Vec<f64>>();
    let average = only_scores.iter().sum::<f64>() / only_scores.len() as f64;

    println!("Got comparison scores of {:?} for website {}. average = {average}", only_scores, site.url);

    let all_changed = only_scores.into_iter().all(|s| s < 0.98);
    let most_similar = screenshot_scores.into_iter()
        .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap())
        .expect("no screenshots?");

    site.last_image = Some(most_similar.1);

    // if get css of page then it always has shop or store or whatever
    let text = page.evaluate("document.body.outerHTML").await?.into_value::<String>()?.to_lowercase();

    let mut merch_newly_detected = MERCH_KEYWORDS.iter()
        .any(|k| text.contains(k));

    if site.merch_already_detected {
        merch_newly_detected = false;
    } else {
        site.merch_already_detected = merch_newly_detected;
    }

    if !all_changed && !merch_newly_detected {
        if site.total_cooldowns != 0 {
            site.total_cooldowns -= 1;
        }

        site.changes_stacking = 0;

        return Ok(());
    }

    if first_run {
        return Ok(());
    }

    // page.save_screenshot(ScreenshotParams::builder().omit_background(true).build(), format!("test_{}.png", site.url.get(10..12).unwrap())).await?;

    let (priority, message) = if merch_newly_detected {
        (1, format!("Detected merch! Difference rating avg of {average}"))
    } else {
        (0, format!("Difference rating avg of {average}"))
    };

    if site.total_runs > 3 && site.should_notify() {
        notify(site, priority, &message).await;
    }

    Ok(())
}

async fn create_screenshot(page: &Page, site: &mut WebsiteData, last_image: &Option<RgbImage>) -> anyhow::Result<(f64, RgbImage)> {
    page.goto(&site.url).await?;
    page.wait_for_navigation().await?;

    let new_screenshot_bytes = page.screenshot(ScreenshotParams::builder().omit_background(true).build()).await?;

    let t = task::block_in_place(move || -> anyhow::Result<(f64, RgbImage)> {
        let screenshot_image = image::load_from_memory(&new_screenshot_bytes)?.into_rgb8();

        let comparison = match last_image {
            Some(ref last_image) => image_compare::rgb_hybrid_compare(&screenshot_image, last_image)?.score,
            None => 1.0,
        };

        Ok((comparison, screenshot_image))
    })?;

    Ok(t)
}

async fn notify(
    website: &WebsiteData,
    priority: i8,
    message: &str,
) {
    println!("Notifying for {}...", website.url);

    let duration_since_epoch = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    let now = duration_since_epoch.as_secs();

    let message = MessageBuilder::new("ucj95tt4g9nwx5zjacepr2brsyi3ot", "a4sijp87u6n8sr7otzvg3r6jbdm4h6", message)
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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use chromiumoxide::{Browser, Page};
    use chromiumoxide::browser::BrowserConfigBuilder;
    use chromiumoxide::page::ScreenshotParams;
    use futures::StreamExt;
    use tokio::task;

    #[tokio::test]
    async fn test_fucking_chrome_oxide() {
        let (browser, mut handler) = Browser::launch(
            BrowserConfigBuilder::default()
                .request_timeout(Duration::from_secs(5))
                // .user_data_dir("C:\\Users\\Cooper\\Downloads")
                .build()
                .unwrap()
        ).await.unwrap();

        println!("past browser launch");

        let _ = task::spawn(async move {
            while let Some(h) = handler.next().await {
                h.unwrap();
            }
        });


        let _ = browser.new_page("https://www.google.com").await.unwrap();

        println!("past page");
    }

    #[tokio::test]
    async fn test_ka_website_comparison() {
        let (browser, mut handler) = Browser::launch(
            BrowserConfigBuilder::default()
                .request_timeout(Duration::from_secs(5))
                .build()
                .unwrap()
        ).await.unwrap();

        let _ = task::spawn(async move {
            while let Some(h) = handler.next().await {
                h.unwrap();
            }
        });

        let page = browser.new_page("about:blank").await.unwrap();
        page.set_user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/116.0.0.0 Safari/537.36").await.unwrap();

        async fn navigate(p: &Page) -> Vec<u8> {
            let page = p.goto("https://www.kevinabstract.co").await.unwrap();
            p.wait_for_navigation().await.unwrap();
            page.screenshot(ScreenshotParams::builder().omit_background(true).build()).await.unwrap()
        }

        let first_image = navigate(&page).await;
        let second_image = navigate(&page).await;
        let third_image = navigate(&page).await;

        println!("past page");

        task::spawn_blocking(move || {
            let first_image = image::load_from_memory(first_image.as_slice()).unwrap().into_rgb8();
            let second_image = image::load_from_memory(second_image.as_slice()).unwrap().into_rgb8();
            let third_image = image::load_from_memory(third_image.as_slice()).unwrap().into_rgb8();


            let comps = [
                image_compare::rgb_hybrid_compare(&first_image, &second_image).unwrap().score,
                image_compare::rgb_hybrid_compare(&first_image, &third_image).unwrap().score,
                image_compare::rgb_hybrid_compare(&second_image, &third_image).unwrap().score,
            ];

            dbg!(comps);

            for comp in comps {
                assert!(comp > 0.98);
            }
        }).await.unwrap();
    }
}