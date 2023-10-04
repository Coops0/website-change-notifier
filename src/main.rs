use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chromiumoxide::{Browser, Page};
use chromiumoxide::browser::BrowserConfigBuilder;
use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;
use chromiumoxide::page::ScreenshotParams;
use dotenv_codegen::dotenv;
use futures::StreamExt;
use image::RgbImage;
use pushover_rs::{MessageBuilder, send_pushover_request};
use tokio::task;
use tokio::time::sleep;

use website_data::{wd, WebsiteData};

mod website_data;

/// set this to = &[]; if you don't want special notifications if merch is newly detected
const MERCH_KEYWORDS: &[&str] = &[
    "merch",
    "store",
    "shop",
    "buy",
    "clothing",
    "clothes",
];

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    /// **EDIT HERE**
    let mut sites: Vec<WebsiteData> = vec![
        wd("https://www.kevinabstract.co")
            .add_script("document.body.style.background='black';")
            .remove_elements(["footer"])
            .build(),
        wd("https://luckyedwards.com")
            .add_script("document.body.style.background='black';")
            .remove_elements(["footer"])
            .build(),
        wd("https://blonded.co/")
            .remove_elements(["video", ".js-header-date-time", ".FooterNotice", "img", "iframe", ".Video__pusher", "#shopify-section-section-footer"])
            .build(),
        wd("https://jid.manheadmerch.com/")
            // this is so dumb lmao, reconstruct page based only product list cause this website fucking sucks
            .add_script(r#"document.documentElement.setHTML(Array.from(document.querySelectorAll(".col-sm-6")).map(t=>{let e=t.getAttribute("data-alpha"),l=t.getAttribute("data-price");return e&&l?`${e}-${l}`:null}).filter(t=>t).join(", "));"#)
            .build(),
        wd("https://sabapivot.store/collections/all")
            .remove_elements(["img"])
            .build(),
        wd("https://videostore.world/").build(),
        wd("https://shop.holidaybrand.co/").build(),
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

    loop {
        println!("--- CYCLE START ---");

        for site in &mut sites {
            if let Err(e) = check_site(&page, site).await {
                eprintln!("Error checking site {} -> {e:?}", site.url());
            }
        }

        println!("--- CYCLE END ---");

        page.goto("about:blank").await?;

        sleep(Duration::from_secs(25)).await;
    }
}

async fn check_site(page: &Page, site: &mut WebsiteData) -> anyhow::Result<()> {
    if !site.should_website_request() {
        return Ok(());
    }

    site.run();

    let first_run = site.last_image.is_none();
    let last_image = site.last_image.take();

    // check if the site changed, if it did change check up to the max confirms times
    let mut screenshot_scores = vec![];
    for _ in 0..site.max_confirms() {
        let result = create_screenshot(page, site, &last_image).await?;
        if result.0 > site.threshold() {
            screenshot_scores.push(result);
            break;
        }

        screenshot_scores.push(result);
        sleep(Duration::from_millis(250)).await;
    }

    let only_scores = screenshot_scores.iter().map(|(s, _)| *s).collect::<Vec<f64>>();
    let average = only_scores.iter().sum::<f64>() / only_scores.len() as f64;

    println!("{} -> avg={average},all={:?}", site.url(), only_scores);

    let all_changed = only_scores.into_iter().all(|s| s < site.threshold());
    let most_similar = screenshot_scores.into_iter()
        .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap())
        .expect("no screenshots?");

    site.last_image = Some(most_similar.1);

    // if get css of page then it always has shop or store or whatever
    let text = page.evaluate("document.body.outerHTML").await?.into_value::<String>()?.to_lowercase();

    let mut merch_newly_detected = MERCH_KEYWORDS.iter()
        .any(|k| text.contains(k));

    // if merch is already detected before, its not newly detected, otherwise set the merch already detected struct value to the new one
    if site.merch_already_detected {
        merch_newly_detected = false;
    } else {
        site.merch_already_detected = merch_newly_detected;
    }

    // nothing happened, run some stuff to ease off cooldown
    if !all_changed && !merch_newly_detected {
        site.nothing_changed();
        return Ok(());
    }

    if first_run {
        return Ok(());
    }

    let message = format!("Found changes on {} with an average difference rating of {average}.{}", site.url(), if merch_newly_detected { "MERCH DETECTED!" } else { "" });

    if site.get_runs() > 3 && site.should_send_notification() {
        notify(site, if merch_newly_detected { 1 } else { 0 }, &message).await;
    }

    Ok(())
}

async fn create_screenshot(page: &Page, site: &mut WebsiteData, last_image: &Option<RgbImage>) -> anyhow::Result<(f64, RgbImage)> {
    page.goto(site.url()).await?;
    page.wait_for_navigation().await?;

    // run all scripts
    for script in site.scripts() {
        let _ = page.evaluate(script.as_str()).await;
    }

    if site.wait() != 0 {
        sleep(Duration::from_millis(site.wait())).await;
    }

    let new_screenshot_bytes = if let Some(selector) = site.screenshot_selector() {
        page.find_element(selector)
            .await?
            .screenshot(CaptureScreenshotFormat::Png)
            .await?
    } else {
        page.screenshot(ScreenshotParams::builder()
            .omit_background(true)
            .full_page(true)
            .build()
        ).await?
    };

    // let a: &[u8] = new_screenshot_bytes.as_ref();
    // tokio::fs::write(format!("test_{}.png", site.url.get(13..16).unwrap()), a).await?;

    // compare with a few special stuff
    let t = task::block_in_place(move || -> anyhow::Result<(f64, RgbImage)> {
        let screenshot_image = image::load_from_memory(&new_screenshot_bytes)?.into_rgb8();

        let comparison = match last_image {
            // if the function fails, then that means the image sizes were different, which means the site was 100% updated
            Some(ref last_image) => image_compare::rgb_hybrid_compare(&screenshot_image, last_image).map(|r| r.score).unwrap_or(0.0),
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
    println!("Notifying for {}...", website.url());

    let duration_since_epoch = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    let now = duration_since_epoch.as_secs();

    let message = MessageBuilder::new(dotenv!("PUSHOVER_USER_KEY"), dotenv!("PUSHOVER_APP_TOKEN"), message)
        .set_title("Website Change Detected")
        .set_url(website.url(), None)
        .add_device("iphone")
        .set_timestamp(now)
        .set_priority(priority)
        .build();

    if let Err(e) = send_pushover_request(message).await {
        eprint!("Error sending message {e:?}");
    }
}