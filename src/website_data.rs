use std::fmt::Debug;

use image::RgbImage;
use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct WebsiteDataConfig {
    url: String,

    /// add a js script to run when the site loads
    #[serde(rename = "scripts")]
    scripts: Option<Vec<String>>,
    /// capture a specific element instead of the whole page (don't use elements that overflow page)
    selector: Option<String>,
    /// automatically remove elements when the page loads
    #[serde(rename = "remove")]
    remove_elements: Option<Vec<String>>,
    /// wait x ms before screenshotting to allow dynamic page to load
    #[serde(default)]
    wait: u64,
    /// when to notify of the change of the site from 0-1, with 0 being totally different, and 1 being the exact same
    #[serde(default = "WebsiteDataConfig::default_threshold")]
    threshold: f64,
    /// after noticing a change, how many times should refresh & verify that the site actually changed
    #[serde(default = "WebsiteDataConfig::default_confirmations")]
    confirmations: u32,
}

impl WebsiteDataConfig {
    fn default_threshold() -> f64 {
        0.995
    }

    fn default_confirmations() -> u32 {
        3
    }
}

impl WebsiteDataConfig {
    fn format_script(script: String) -> String {
        format!("()=>{{{}}}", script)
    }

    fn format_remove_elements(elements: Vec<String>) -> String {
       WebsiteDataConfig::format_script(
           format!("document.querySelectorAll('{}')?.forEach(a => a?.remove());", elements.join(", "))
       )
    }

    pub fn build(mut self) -> WebsiteData {
        if self.url.is_empty() {
            panic!("require url to be non blank");
        }

        if self.confirmations == 0 {
            panic!("max confirms has to be >0");
        }

        if self.threshold < 0.0 || self.threshold > 1.0 {
            panic!("threshold has to be > 0 & < 1")
        }

        let mut scripts = vec![];

        if let Some(elements) = self.remove_elements.take() {
            scripts.push(WebsiteDataConfig::format_remove_elements(elements));
        }

        if let Some(new_scripts) = self.scripts.take() {
            for script in new_scripts {
                scripts.push(WebsiteDataConfig::format_script(script));
            }
        }

        WebsiteData {
            url: self.url,
            scripts,
            screenshot_selector: self.selector,
            wait: self.wait,
            threshold: self.threshold,
            max_confirms: self.confirmations,

            last_image: None,
            merch_already_detected: false,
            changes_stacking: 0,
            current_cooldown: 0,
            total_cooldowns: 0,
            total_runs: 0,
        }
    }
}

#[derive(Debug)]
pub struct WebsiteData {
    url: String,
    scripts: Vec<String>,
    screenshot_selector: Option<String>,
    wait: u64,
    threshold: f64,
    max_confirms: u32,

    pub last_image: Option<RgbImage>,
    pub merch_already_detected: bool,

    /// in a row, count the number of times i have been texted, used for cooldown
    changes_stacking: u8,
    /// if notified consecutively >= 4 times, add a cooldown that increases more with each cooldown
    current_cooldown: u16,
    /// counts the number of cooldowns recieved, decreases one per successful blank/cycle
    total_cooldowns: u32,

    total_runs: u64,
}

// <editor-fold desc="Website data helper functions">
impl WebsiteData {
    pub fn scripts(&self) -> &Vec<String> {
        &self.scripts
    }
    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn screenshot_selector(&self) -> &Option<String> {
        &self.screenshot_selector
    }

    pub fn wait(&self) -> u64 {
        self.wait
    }

    pub fn threshold(&self) -> f64 {
        self.threshold
    }

    pub fn max_confirms(&self) -> u32 {
        self.max_confirms
    }

    pub fn get_runs(&self) -> u64 {
        self.total_runs
    }
}
// </editor-fold>

impl WebsiteData {
    pub fn run(&mut self) {
        self.total_runs += 1;
    }

    pub fn nothing_changed(&mut self) {
        if self.total_cooldowns != 0 {
            self.total_cooldowns -= 1;
        }

        self.changes_stacking = 0;
    }

    // should run init request, basically check if its on a cooldown
    pub fn should_website_request(&mut self) -> bool {
        if self.current_cooldown == 0 {
            return true;
        }

        self.current_cooldown -= 1;
        self.current_cooldown == 0
    }

    // INFERS CHANGES ARE DETECTED, if they are then calculate if this notification should result in a cooldown instead
    pub fn should_send_notification(&mut self) -> bool {
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