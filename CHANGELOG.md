# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- **Ctrl+L address bar**: now editable; paste any URL from an allowed domain to navigate directly in the app. Invalid domains show an inline error and keep the dialog open.

## [0.2.0] - 2026-03-15

### Added

- **Native KDE file dialogs**: set `GTK_USE_PORTAL=1` so file chooser dialogs use xdg-desktop-portal, showing KDE's native Dolphin-based picker on Plasma instead of GTK's
- **Download file chooser**: GTK save dialog on download; cancelling the dialog cancels the download
- **Raise existing window**: second `open` invocation raises the running window via Unix socket IPC instead of erroring
- **Keyboard shortcuts**: Ctrl+Q quits, Ctrl+W hides to tray (or quits if tray is disabled); Alt+Left/Right for back/forward; Ctrl+R reloads; Ctrl+Shift+R hard-reloads (cache bypass); Ctrl+L shows current URL in a copyable dialog
- **`excluded_domains`**: list of domains that always open in the system browser, even if they match `allowed_domains`; useful for e.g. excluding `meet.google.com` from a workspace app
- **Browser extension injection**: when `inject` fields are set with a browser backend (Brave/Chrome/Chromium), an unpacked MV3 Chrome extension is auto-generated and loaded via `--load-extension`; supports `inject.css`, `inject.js`, `inject.css_file`, `inject.js_file`; Brave and Chromium are silent, Chrome shows a developer-mode notice on every launch
- **Fullscreen polyfill**: `requestFullscreen()`/`exitFullscreen()` and all webkit-prefixed variants are intercepted and mapped to native window fullscreen; fixes fullscreen buttons in video players, YouTube, etc.
- **beforeunload support**: synthetic beforeunload event dispatch with native GTK confirmation dialog
- **Reinstall by name**: `install <name>` looks up the existing config from the XDG config dir
- **Better icons**: icon fetcher now checks web app manifest and apple-touch-icon for larger images (192-512px); all icons normalized to PNG
- **Raise on notification click**: clicking a system notification raises the app window
- **Wayland app-id**: `g_set_prgname` sets the correct app-id for alt-tab icon matching in webview mode
- **Browser backend WM_CLASS**: `StartupWMClass` in `.desktop` files now matches the Chromium-predicted Wayland app_id

### Fixed

- beforeunload dialog no longer fires on every close when no `beforeunload` listener is registered on the page
- Tray window restore on Wayland: call `gtk_window_present()` and force resize to recover compositor state
- All exit paths use `process::exit(0)` to avoid shutdown delay from notification action listener threads
- All popups denied and opened in system browser (prevents unmanaged GTK windows)
- flock error handling: `EWOULDBLOCK` (already running) distinguished from real I/O errors
- Manifest icon URLs resolved against manifest URL, not page URL
- `--class` arg in browser launch now matches `StartupWMClass`
- HTML icon parser: `to_ascii_lowercase()` for byte-index slicing (prevents panics on non-ASCII HTML)
- `chromium_app_name_from_url`: strip port, query, fragment, and userinfo; handle IPv6 brackets; split authority at first of `/?#`
- `create_raise_listener` failures logged under `--debug` instead of silently dropped
- `resolve_url` handles pathless base URLs and strips query/fragment before computing relative base
- `evaluate_script(BEFOREUNLOAD_CHECK)` errors exit immediately instead of leaving window unable to close
- `fetch_largest_manifest_icon` skips bad icon entries instead of aborting the search
- `save_as_png` falls back to original format on decode failure (e.g. SVG)
- Download dir uses `directories::UserDirs::download_dir()` instead of manual env var lookup

### Changed

- Local builds now show `git describe` version (e.g. `0.1.3-15-gabcdef`) instead of `0.1.0`
- Added `examples/notion.yaml` (tested, works on WebKitGTK)

## [0.1.1] - 2026-03-12

### Added

- Install instructions in README
- CI workflow (check, test, clippy) on push and PRs
- Release workflow with test gate
- Cache apt packages in CI for faster builds

## [0.1.0] - 2026-03-12

Initial release.

### Added

- Turn any website into a standalone desktop app with a YAML config file
- Profile isolation (separate cookies, storage, and cache per profile)
- CSS and JS injection per app (inline and file references)
- Ad and tracker blocking via JS interception (~3500 domain blocklist)
- System notifications forwarded to desktop via libnotify
- System tray icon with minimize-to-tray
- `.desktop` file generation with auto-fetched favicons
- External links open in default browser, configurable allowed domains
- Global `defaults.yaml` for shared settings across apps
- Single instance enforcement per app+profile (flock-based)
- Window position and size persistence across sessions
- Browser backend fallback (Brave/Chrome/Chromium `--app` mode) for sites incompatible with WebKitGTK
- Navigator spoofing (`navigator.vendor`, `navigator.platform`, `window.chrome`)
- URL scheme handler registration via `.desktop` files
- Interactive profile picker when multiple profiles exist
- `list`, `install`, `uninstall`, and `clear-cache` commands

### Requirements

- Linux (Debian/Ubuntu)
- libwebkit2gtk-4.1-0, libgtk-3-0, libxdo3, libayatana-appindicator3-1
