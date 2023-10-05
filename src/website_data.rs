use std::fmt::{Debug, Formatter};

use image::RgbImage;
use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct WebsiteDataBuilder {
    url: String,

    #[serde(skip)]
    inner_scripts: Option<Vec<String>>,

    screenshot_selector: Option<String>,
    #[serde(default)]
    wait: u64,
    #[serde(default = "WebsiteDataBuilder::default_threshold")]
    threshold: f64,
    #[serde(default = "WebsiteDataBuilder::default_max_confirms")]
    max_confirms: u32,

    // only used to be converted later

    // Automatically converted normally, this is used for toml deserialization
    #[serde(rename = "remove-elements")]
    remove_elements: Option<Vec<String>>,
    // used for deserialization, needs to still be handled to format
    #[serde(rename = "scripts")]
    scripts: Option<Vec<String>>,
}

impl Default for WebsiteDataBuilder {
    fn default() -> Self {
        Self {
            url: String::new(),
            inner_scripts: None,
            scripts: None,
            screenshot_selector: None,
            wait: 0,
            threshold: WebsiteDataBuilder::default_threshold(),
            max_confirms: WebsiteDataBuilder::default_max_confirms(),
            remove_elements: None,
        }
    }
}

impl From<WebsiteDataBuilder> for WebsiteData {
    fn from(value: WebsiteDataBuilder) -> Self {
        value.build()
    }
}

/// shorthand of creating builder w url
pub fn wd(url: &str) -> WebsiteDataBuilder {
    WebsiteDataBuilder {
        url: url.to_string(),
        ..WebsiteDataBuilder::default()
    }
}

impl WebsiteDataBuilder {
    fn default_threshold() -> f64 {
        0.99
    }

    fn default_max_confirms() -> u32 {
        3
    }
}

impl WebsiteDataBuilder {
    /// sets url of the site
    pub fn url<S: ToString>(mut self, url: S) -> Self {
        self.url = url.to_string();
        self
    }

    fn inner_add_script(&mut self, script: String) {
        self.inner_scripts
            .get_or_insert_with(Vec::new)
            .push(format!("()=>{{{}}}", script));
    }

    /// add a javascript script to run when the site loads
    pub fn add_script<S: ToString>(mut self, script: S) -> Self {
        self.inner_add_script(script.to_string());
        self
    }

    /// capture a specific element instead of the whole page (prolly don't use it kinda sucks)
    pub fn selector<S: ToString>(mut self, selector: S) -> Self {
        self.screenshot_selector = Some(selector.to_string());
        self
    }

    fn inner_remove_elements_script(elements: Vec<String>) -> String {
        format!("document.querySelectorAll('{}')?.forEach(a => a?.remove());", elements.join(", "))
    }

    /// automatically remove elements when the page loads
    pub fn remove_elements<S: Into<String>, V: Into<Vec<S>>>(self, elements: V) -> Self {
        self.add_script(
            WebsiteDataBuilder::inner_remove_elements_script(
                elements.into()
                    .into_iter()
                    .map(Into::into)
                    .collect()
            )
        )
    }


    /// wait x ms before screenshotting to allow shit to load
    pub fn wait(mut self, wait: u64) -> Self {
        self.wait = wait;
        self
    }

    /// when to notify of the change of the site from 0-1, with 0 being totally different, and 1 being the exact same
    pub fn threshold(mut self, threshold: f64) -> Self {
        self.threshold = threshold;
        self
    }

    /// after noticing a change, how many times should refresh & verify that the site actually changed
    pub fn confirmations(mut self, confirms: u32) -> Self {
        self.max_confirms = confirms;
        self
    }

    pub fn build(mut self) -> WebsiteData {
        if self.url.is_empty() {
            panic!("require url to be non blank");
        }

        if self.max_confirms == 0 {
            panic!("max confirms has to be >0");
        }

        if self.threshold < 0.0 || self.threshold > 1.0 {
            panic!("threshold has to be > 0 & < 1")
        }

        if let Some(elements) = self.remove_elements.take() {
            self.inner_add_script(WebsiteDataBuilder::inner_remove_elements_script(elements));
        }

        if let Some(scripts) = self.scripts.take() {
            for script in scripts {
                self.inner_add_script(script);
            }
        }

        WebsiteData {
            url: self.url,
            scripts: self.inner_scripts,
            screenshot_selector: self.screenshot_selector,
            wait: self.wait,
            threshold: self.threshold,
            max_confirms: self.max_confirms,

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
    scripts: Option<Vec<String>>,
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

impl WebsiteData {
    pub fn scripts(&self) -> &Option<Vec<String>> {
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