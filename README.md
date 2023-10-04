# Website Change Notifier
## Send a pushover notification when a website visiblity changes

Create a .env file with "PUSHOVER_USER_KEY" and "PUSHOVER_APP_TOKEN"

In the main file, edit the `sites` array to your own sites. All functions are documented in the code as well.

```rust
    wd("website url")
        .add_script("document.querySelector('body').style.background = 'blue';") // add a js script to run when the site loads
        .selector(".product-list") // capture a specific element instead of the whole page (don't use elements that overflow page)
        .remove_elements([".cookie-consent-banner", "nav", "#button-1"]) // automatically remove elements when the page loads
        .wait(300) // wait x ms before screenshotting to allow dynamic page to load
        .threshold(0.985) // when to notify of the change of the site from 0-1, with 0 being totally different, and 1 being the exact same
        .confirmations(4) // after noticing a change, how many times should refresh & verify that the site actually changed
        .build()
```

- It detects if merch is newly detected and will send a special notification (can be turned off).
- Automatic cooldown/backoff system to prevent being spammed if something goes wrong.