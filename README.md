# please-make-me-an-app

Dotfiles for your web apps.

Turn any website into a standalone desktop app with a YAML config file. No Electron, no bundled browser, no build step.

```sh
please-make-me-an-app open whatsapp.yaml
```

## Why

Browser tabs don't cut it when you live in a web app 8 hours a day. You want your own window, your own taskbar icon, and the ability to tweak things. But everything that does this either bundles Chromium (80+ MB per app) or requires a build step to produce a binary.

This takes a different approach: write a YAML config, run a command, get a window. Change the config, re-run. Keep your app configs in version control alongside your dotfiles.

## Why not just use PWAs?

PWAs are the right answer for most people. If "Install as app" from Chrome works for you, use that.

Where they fall short:

- **No automation.** You can't script a PWA install from the terminal. You can hand-write `.desktop` files with browser flags and hardcoded URLs, but in practice these are fragile. The "app" gets grouped with your main browser in the taskbar, custom icons get ignored, and you're one browser update away from something breaking.
- **No profile isolation.** PWAs share your browser's session. You can't run two separate Google accounts as two separate apps with their own cookies and storage.
- **No page customization.** No way to inject CSS to hide a distracting sidebar or JS to add a keyboard shortcut.
- **Browser-dependent.** The good PWA implementation is Chrome/Edge only. Firefox's support is experimental and Windows-only. Safari's "Add to Dock" is macOS-only and limited.
- **Shared process tree.** PWAs run inside your browser's process model. They compete for memory with your tabs and don't show up as independent apps in system monitors.

If any of that bothers you, read on 🚀

## What makes it different

There's no build step. Tools like Pake compile a binary per app. Here, the config *is* the app. Edit the YAML and re-run. Each profile gets its own cookies, storage, and cache, so you can run two Gmail accounts side by side without them leaking into each other.

You can inject CSS and JS per app to hide elements, add shortcuts, or rearrange layouts. Only one instance per app+profile runs at a time (flock-based), so launching the same app twice won't give you duplicates.

It uses your system's WebKitGTK instead of shipping its own browser engine, so the binary is small and RAM usage stays reasonable.

## Quick start

### Requirements

Linux only for now (Debian/Ubuntu). Sorry, macOS/Windows folks 🙈

```sh
sudo apt install libwebkit2gtk-4.1-0 libgtk-3-0 libxdo3 libayatana-appindicator3-1
```

### Install

```sh
curl -L https://github.com/fgrehm/please-make-me-an-app/releases/latest/download/please-make-me-an-app-x86_64-linux.tar.gz | tar xz -C ~/.local/bin please-make-me-an-app
```

This assumes `~/.local/bin` is on your `$PATH` (it is by default on most distros). If not, use `/usr/local/bin` or any other directory on your `$PATH`.

### Usage

```sh
# Open an app
please-make-me-an-app open config.yaml

# Open with a specific profile
please-make-me-an-app open config.yaml --profile work

# Open with debug logging (prints UA, config, data dir to stderr)
please-make-me-an-app open config.yaml --debug

# Install to your desktop launcher
please-make-me-an-app install config.yaml

# List installed apps
please-make-me-an-app list

# Clear cached web data
please-make-me-an-app clear-cache gmail
please-make-me-an-app clear-cache gmail --profile work

# Uninstall (--purge removes profile data and icon too)
please-make-me-an-app uninstall gmail --purge

# Uninstall everything (interactive confirmation)
please-make-me-an-app uninstall --all
```

### Example config

```yaml
name: whatsapp
url: https://web.whatsapp.com
window:
  title: WhatsApp
  width: 1100
  height: 750
user_agent: "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36"
notifications: true
tray:
  enabled: true
  minimize_to_tray: true
allowed_domains:
  - whatsapp.com
  - whatsapp.net
  - wa.me
```

See `examples/` for more configs (Gmail, Pomofocus, Claude).

## What else it does

Ad and tracker blocking is built in (~3500 domain blocklist, patching fetch/XHR/Image at the JS level). Web notifications get forwarded to your desktop via libnotify. There's an optional system tray icon with minimize-to-tray. The `install` command fetches the site's favicon and generates a `.desktop` file. Off-domain links open in your default browser. You can also set up a global `defaults.yaml` to share window size, user agent, and inject rules across all your apps.

## Config reference

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `name` | string | required | App identifier (letters, numbers, hyphens, underscores) |
| `url` | string | required | URL to open (must start with http:// or https://) |
| `backend` | string | `"webview"` | Backend to use: `webview`, `brave`, `chrome`, or `chromium` |
| `window.title` | string | `"please-make-me-an-app"` | Window title |
| `window.width` | int | `1200` | Window width in pixels |
| `window.height` | int | `800` | Window height in pixels |
| `window.remember_position` | bool | `false` | Restore window position and size from last session |
| `profiles` | list | `[]` | Named profiles for session isolation |
| `inject.css` | string | | Inline CSS to inject |
| `inject.js` | string | | Inline JS to inject |
| `inject.css_file` | path | | CSS file to inject (relative to config) |
| `inject.js_file` | path | | JS file to inject (relative to config) |
| `user_agent` | string | | Custom user agent string |
| `navigator.vendor` | string | | Override `navigator.vendor` in JS |
| `navigator.platform` | string | | Override `navigator.platform` in JS |
| `navigator.chrome` | bool | `false` | Inject `window.chrome = { runtime: {} }` to spoof Chrome presence |
| `clipboard` | bool | `true` | Allow clipboard access |
| `adblock` | bool | `true` | Block ads and trackers |
| `adblock_extra` | path | | Additional blocklist file (one domain per line) |
| `notifications` | bool | `true` | Forward web notifications to system |
| `tray.enabled` | bool | `false` | Show system tray icon |
| `tray.minimize_to_tray` | bool | `false` | Close button hides window instead of quitting |
| `allowed_domains` | list | `[]` | Domains that stay in the app (others open in browser) |
| `open_external_links` | bool | `true` | Open blocked external navigations in the system browser. Set to `false` for apps that auto-navigate to cross-domain URLs you don't want landing in your browser. |
| `url_schemes` | list | `[]` | URL schemes to register as handler for (e.g., `tel`, `mailto`). Adds `MimeType=x-scheme-handler/...` to the `.desktop` file. |

### Tip: user agent and navigator spoofing

Some sites (WhatsApp, Slack, etc.) detect webview user agents and refuse to load. Set `user_agent` to a Chrome string to work around this. When a site also checks `navigator.vendor`, `navigator.platform`, or `window.chrome`, use the `navigator.*` config fields to match:

```yaml
user_agent: "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/133.0.0.0 Safari/537.36"
navigator:
  vendor: "Google Inc."
  platform: "Linux x86_64"
  chrome: true
```

## How it works

```
                           webview     wry + tao --> native WebKitGTK window
                          /                |
YAML config --> CLI (clap)                 +--> Profile-isolated data directory
                          \                +--> CSS/JS injection on page load
                           browser         +--> Ad blocking via JS API interception
                              |            +--> Notification forwarding via IPC
                              v            +--> System tray via tray-icon
               Brave/Chrome/Chromium --app mode
                              |
                              +--> Profile isolation via --user-data-dir
```

All data follows the XDG Base Directory Spec:

- `$XDG_CONFIG_HOME/please-make-me-an-app/apps/<name>.yaml` -- per-app configs
- `$XDG_DATA_HOME/please-make-me-an-app/profiles/<app>/<profile>/webview-data/` -- isolated WebKitGTK data (webview backend)
- `$XDG_DATA_HOME/please-make-me-an-app/profiles/<app>/<profile>/chromium-data/` -- isolated Chromium data (browser backend)
- `$XDG_DATA_HOME/please-make-me-an-app/icons/<name>.png` -- cached favicons
- `~/.local/share/applications/pmma-<app>.desktop` -- launcher entries (no profiles)
- `~/.local/share/applications/pmma-<app>--<profile>.desktop` -- per-profile launcher entries

The `install` command copies your config to the XDG config directory and records the full path to the binary in the `.desktop` file's `Exec=` line. The launcher entry keeps working even if you move the original config or the binary isn't on your `$PATH`.

For apps with profiles, `install` generates one launcher entry per profile. Each gets its own name ("Gmail (work)", "Gmail (personal)"), its own log file, and the `--profile` flag baked into the `Exec=` line. Re-running `install` cleans up old entries, so adding or removing profiles is safe.

## Known limitations

- Linux only. No macOS or Windows yet (PR welcome!)
- No WebAuthn/YubiKey. WebKitGTK does not implement the WebAuthn API. Use `backend: brave` (or `chrome`/`chromium`) as a fallback.
- Sites behind Cloudflare Turnstile (claude.ai, itch.io) don't work in the webview. Use a browser backend instead.
- Ad blocking is JS-level. Cannot block `<script>` tags already in the HTML.
- Browser backend does not support CSS/JS injection, system tray, or ad blocking config (Brave has its own built-in blocker).
- Always-on-top is broken on KDE Plasma. Use the title bar right-click menu instead.
- Drag-and-drop may fail on Wayland due to a WebKitGTK bug.
- No Chrome extensions in webview mode. Apps that depend on one for core functionality (Loom recorder, Grammarly) won't work.

See [docs/known-limitations.md](docs/known-limitations.md) for details.

## Building

Requires Rust and WebKitGTK build dependencies (see `.devcontainer/`).

```sh
cargo build --release
# Binary: target/release/please-make-me-an-app
```

## How this was built

I'm not a Rust developer. This whole thing was built with AI assistance. It works, it has tests, but don't expect idiomatic Rust. PRs from actual Rustaceans are welcome ❤️

## License

MIT
