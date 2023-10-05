# Website Change Notifier
## Send a pushover notification when a website visibly changes

Create a .env file with "PUSHOVER_USER_KEY" and "PUSHOVER_APP_TOKEN".

Create a sites.toml file, there is an example in the repository.
```toml
[[sites]]
url = "https://www.kevinabstract.co"
scripts = ["document.body.style.background='black';"] # add a js script to run when the site loads
selector = ".product-list" # capture a specific element instead of the whole page (don't use elements that overflow page)
remove = [".cookie-consent-banner", "nav", "#button-1"] # automatically remove elements when the page loads
wait = 300 # wait x ms before screenshotting to allow dynamic page to load
threshold = 0.985 # when to notify of the change of the site from 0-1, with 0 being totally different, and 1 being the exact same
confirmations = 4 # after noticing a change, how many times should refresh & verify that the site actually changed

[[sites]]
url = "https://blonded.co"
```

- It detects if merch is newly detected and will send a special notification (can be turned off).
- Automatic cooldown/backoff system to prevent being spammed if something goes wrong.
