# Changelog

## v0.2.0

Real-world usage improvements across all core features.

### New features

- **Download file chooser**: GTK save dialog on download; cancelling the dialog cancels the download
- **Raise existing window**: second `open` invocation raises the running window via Unix socket IPC instead of erroring
- **Keyboard shortcuts**: Ctrl+Q quits, Ctrl+W hides to tray (or quits if tray is disabled)
- **beforeunload support**: synthetic beforeunload event dispatch with native GTK confirmation dialog
- **Reinstall by name**: `install <name>` looks up the existing config from the XDG config dir
- **Better icons**: icon fetcher now checks web app manifest and apple-touch-icon for larger images (192-512px); all icons normalized to PNG
- **Raise on notification click**: clicking a system notification raises the app window
- **Wayland app-id**: `g_set_prgname` sets the correct app-id for alt-tab icon matching in webview mode
- **Browser backend WM_CLASS**: `StartupWMClass` in `.desktop` files now matches the Chromium-predicted Wayland app_id

### Fixes

- Tray window restore on Wayland: call `gtk_window_present()` and force resize to recover compositor state
- Tray quit no longer hangs: `process::exit(0)` bypasses blocked notification threads
- All popups denied and opened in system browser (prevents unmanaged GTK windows)
- flock error handling: `EWOULDBLOCK` (already running) distinguished from real I/O errors
- Manifest icon URLs resolved against manifest URL, not page URL
- `--class` arg in browser launch now matches `StartupWMClass`
- HTML icon parser: `to_ascii_lowercase()` for byte-index slicing (prevents panics on non-ASCII HTML)
- `chromium_app_name_from_url` strips port, query string, and fragment
- `create_raise_listener` failures logged under `--debug` instead of silently dropped

### Other

- Local builds now show `git describe` version (e.g. `0.1.3-15-gabcdef`) instead of `0.1.0`
- Added `examples/notion.yaml` (tested, works on WebKitGTK)

## v0.1.1

- Add install instructions to README
- Add CI workflow (check, test, clippy) on push and PRs
- Add release workflow with test gate
- Cache apt packages in CI for faster builds

## v0.1.0

Initial release.

### Features

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
