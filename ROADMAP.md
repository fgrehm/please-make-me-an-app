# Roadmap

Core features work: webview and browser backends, profiles, CSS/JS injection, ad blocking, notifications, tray, `.desktop` generation, single instance, keyboard shortcuts. The known rough edges are documented in `docs/known-limitations.md`.

---

## Daily driver

The goal is removing friction for using this with several apps every day.

### Event loop: migrate to EventLoopProxy

Currently the event loop polls at 250ms unconditionally so it can catch signals from tray events, keyboard shortcuts, and the raise-window socket thread. This adds up to ~250ms latency on shortcuts and window raise.

The fix is replacing `AtomicBool` flags with `EventLoopProxy::send_event()`, which wakes the loop immediately from any thread. Non-trivial refactor: every signal source (IPC handler, tray receiver, raise listener) needs a proxy clone, and the event type needs to enumerate all signals.

See `docs/known-limitations.md` → Event Loop Polling.

### Devtools toggle

Add a `--devtools` CLI flag (and optionally a `devtools: true` config field) to open the WebKit inspector panel. Useful for debugging CSS/JS injection without a separate browser session. wry supports `with_devtools(true)` and `open_devtools()` behind the `devtools` Cargo feature.

### Native ad blocking

The current JS-level blocker (patching `fetch`, `XMLHttpRequest`, `Image`, `sendBeacon`) cannot block `<script>` tags already in the HTML, CSS `background-image` requests, or `<link rel="preload">`. WebKitGTK's `WebKitUserContentFilter` API provides proper content blocking at the network layer (same mechanism as Safari content blockers), but wry does not expose it.

Options: call the GTK API directly via `gtk-rs`/`webkit2gtk` bindings, or patch wry upstream. See `docs/ad-blocking.md` for prior research.

---

## Ecosystem integration

### Companion browser extension: HTTPS domain routing

The biggest missing piece for daily use: clicking a `https://notion.so/...` link in another app (Slack, email, terminal) opens a browser tab instead of the installed PMMA app.

The fix is a browser extension that intercepts navigation on configured origins and routes them to the local app via native messaging or a `pmma://` scheme handler.

Config sketch:

```yaml
capture_origins:
  - https://www.notion.so
  - https://notion.so
```

See `docs/ideas.md` for the full design. Requires: extension (Chrome/Firefox WebExtension API), native messaging host or scheme handler, `install` registering the origin mapping.

### User-specified extensions for browser backend

Allow browser-mode apps to load user-specified Chrome extensions via an `extensions: [/path/to/unpacked-ext]` config field. Useful for apps like Loom where the extension adds recording UI via content scripts. Toolbar popup extensions are unreachable in `--app` mode; content script extensions still work.

---

## Cross-platform

### macOS

- Backend: WKWebView via wry (no new dependency, already supported)
- App bundle: generate a `.app` bundle and add to Dock via `osascript` or `defaults`
- Profile data: `~/Library/Application Support/` instead of XDG

### Windows

- Backend: WebView2 via wry (already supported, requires WebView2 runtime)
- Shortcut: generate a `.lnk` in `%APPDATA%\Microsoft\Windows\Start Menu\Programs\`
- Profile data: `%LOCALAPPDATA%\` instead of XDG

---

## Stretch

### Binary packaging

Package individual apps as standalone executables with the YAML config embedded. The resulting binary opens the configured app directly, no CLI arguments needed. Useful for sharing a single-file app or adding to autostart.

### Live config reload

Watch the config file for changes and reload CSS/JS injection and window properties without restarting. Useful for iterating on `inject.css`/`inject.js` without re-running `open`.
