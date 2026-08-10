# please-make-me-an-app - Project Context

## What this is

A Rust CLI tool that turns any website into a standalone desktop app using native webviews. Users write a YAML config file per app and run `please-make-me-an-app open <config>` to open the site in its own window. Supports multiple profiles per app for session isolation, CSS/JS injection, ad blocking, system notifications, system tray, and `.desktop` file generation. For sites incompatible with WebKitGTK (Cloudflare Turnstile, WebAuthn), a browser backend launches Brave/Chrome/Chromium in `--app` mode with `--user-data-dir` for profile isolation.

## Tech stack

- **Rust** (2021 edition)
- **wry 0.54** for WebView (WebKitGTK backend on Linux)
- **tao 0.34** for window management
- **clap 4** (derive API) for CLI argument parsing
- **serde** + **serde_yaml_ng 0.10** for config deserialization (not serde_yaml, which is deprecated)
- **ureq 3** for HTTP requests (favicon fetching)
- **directories** for XDG Base Directory paths
- **notify-rust 4** for system notifications via D-Bus
- **tray-icon 0.21** for system tray (with muda menus)
- **image 0.25** (png+ico features) for tray icon loading
- **serde_json** for IPC message parsing
- **libc 0.2** for flock-based single instance enforcement

## Directory layout

### Project source

```
src/
  main.rs         - Entry point, CLI setup, install/list/uninstall commands
  config.rs       - YAML config parsing, validation, global defaults merge, Backend enum
  app.rs          - WebView window creation, event loop, navigation handling
  browser.rs      - Chromium-based browser backend (Brave/Chrome/Chromium --app mode)
  profile.rs      - Profile management, data dirs, single instance locking
  desktop.rs      - .desktop file generation and parsing
  inject.rs       - CSS/JS injection into webview
  icon.rs         - Favicon fetching, caching, format detection
  notification.rs - Web Notification API intercept and system notifications
  tray.rs         - System tray icon with context menu
  adblock.rs      - Ad/tracker blocking via JS API interception
```

### Runtime paths (XDG Base Directory Spec)

- **Config**: `$XDG_CONFIG_HOME/please-make-me-an-app/`
  - `apps/<app-name>.yaml` - Per-app YAML config files (copied here by `install`)
  - `defaults.yaml` - Global defaults merged with per-app configs
- **Data**: `$XDG_DATA_HOME/please-make-me-an-app/`
  - `profiles/<app-name>/<profile-name>/` - Profile root (lock file, window_state.json)
  - `profiles/<app-name>/<profile-name>/webview-data/` - Isolated WebKitGTK data (webview backend)
  - `profiles/<app-name>/<profile-name>/chromium-data/` - Isolated Chromium data (browser backend)
  - `icons/<app-name>.png` - Cached favicons
- **Desktop entries**: `~/.local/share/applications/pmma-<app-name>.desktop` (no profiles) or `pmma-<app-name>--<profile>.desktop` (per-profile)
- **Logs**: `/tmp/pmma-<app-name>.log` (stdout/stderr from .desktop launcher)

## Architecture decisions

- **wry over Electron/CEF**: Native webview means no bundled browser engine. Small binary, fast startup, low memory. Tradeoff: depends on system WebKitGTK version.
- **One config file per app**: Simpler mental model than a monolithic config. Each app is self-contained. Easy to share, version control, or delete individual app configs.
- **Named profiles with separate data dirs**: Each profile maps to a distinct WebKitGTK data directory. This gives full cookie/storage/cache isolation between profiles (e.g., work vs personal Gmail). Each app+profile runs as its own OS process with its own window, tray icon, and flock. The `install` command generates one `.desktop` file per profile (e.g., `pmma-gmail--work.desktop`) with `--profile` baked into the Exec line. Window titles and tray tooltips include the profile name ("Gmail (work)") when profiles are defined. `--` is used as the app/profile separator in desktop filenames, so app and profile names cannot contain consecutive hyphens.
- **CSS/JS injection via wry's initialization scripts**: wry supports injecting scripts before page load. CSS injection is done by wrapping CSS in a `<style>` element via JavaScript.
- **Favicon auto-fetch**: On `install`, fetch the site's favicon via HTTP (parse HTML for `<link rel="icon">`, fall back to `/favicon.ico`), cache locally. Used as the icon in `.desktop` files and system tray.
- **Single instance via flock**: Each app+profile combination uses an exclusive non-blocking `flock()` on a lock file in the profile data dir. Prevents duplicate instances, auto-releases on crash.
- **Popup denial**: All new window requests (`window.open`, `target="_blank"`) are denied. HTTP/HTTPS URLs are opened in the system browser instead. Google redirect URLs (`google.com/url?q=...`) are unwrapped to extract the actual destination before opening. This avoids unmanaged GTK popup windows and provides a better UX for login flows, link clicks, etc.
- **Notification interception**: The Web Notification API is replaced with an IPC shim that forwards to `notify-rust`. Permission is always reported as "granted", so apps that check `Notification.permission` work without prompts.
- **Tray via tray-icon crate**: The tray-icon crate (not tao's built-in) with muda menus. Tray events use a separate channel requiring `ControlFlow::WaitUntil` polling in the event loop.
- **Browser backend fallback**: For sites incompatible with WebKitGTK (Cloudflare Turnstile, WebAuthn), the `backend` config option launches a Chromium-based browser (Brave, Chrome, Chromium) in `--app` mode. Uses `--user-data-dir` for profile isolation and `--class` for WM_CLASS matching. Each backend stores data in its own subdirectory (`webview-data/` or `chromium-data/`) under the profile, so switching backends is safe. Features like CSS/JS injection, ad blocking config, tray, and notification interception don't apply in browser mode (warnings are printed).

## Development workflow

- **Build in devcontainer, run on host.** The devcontainer has all Rust and WebKitGTK build dependencies. Compile inside the container, then run the binary on the host where the display server lives.
- Use `crib exec -- <command>` to run commands inside the devcontainer. Examples:
  - `crib exec -- cargo check`
  - `crib exec -- cargo test`
  - `crib exec -- cargo clippy`
  - `crib exec -- cargo build --release`
- If the container is not running, start it with `crib up`.
- The host machine needs runtime libraries installed: `libwebkit2gtk-4.1-0`, `libgtk-3-0`, `libxdo3`, `libayatana-appindicator3-1`.
- The binary in `target/debug/` or `target/release/` is accessible from the host via the shared workspace mount.
- No UI testing is possible inside the container (no display server).

## Code style

- Keep it simple. Flat module structure, no unnecessary abstractions.
- Use `anyhow` for error handling in the binary (not a library).
- Prefer `thiserror` if custom error types become necessary.
- Use `clap` derive macros for CLI definition.
- Serde derive for all config structs.
- Minimal `unsafe` code: only `libc::flock` for single instance locking.

## Versioning

- The displayed version comes from `git describe --tags --exact-match` at build time (via `build.rs`).
- CI builds from a tag (e.g., `v0.1.2`) get the tag as the version.
- Local builds without a tag on HEAD fall back to the version in `Cargo.toml`.
- `Cargo.toml` version does not need to be bumped for releases. Just push a `v*` tag.
- The `PMMA_VERSION` env var is set by `build.rs` and used in `main.rs` for both `-V` and `--version`.

## Standards and conventions

- **Git**: Conventional Commits with scopes (e.g., `feat(config):`, `fix(webview):`)
- **Config**: YAML format, validated with serde
- **File paths**: XDG Base Directory Spec
- **Exit codes**: 0 success, 1 general error, 2 config/usage error
- **License**: MIT
