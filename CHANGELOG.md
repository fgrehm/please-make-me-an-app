# Changelog

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
