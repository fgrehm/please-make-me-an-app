# Known Limitations

Limitations inherent to WebKitGTK that affect apps built with please-make-me-an-app.

## WebAuthn / FIDO2 / YubiKey

WebKitGTK does not implement the Web Authentication API (WebAuthn). Hardware security keys (YubiKey, SoloKey, etc.) and passkeys will not work for login or two-factor authentication.

This affects sites like Google, GitHub, and any service that requires or prefers WebAuthn for 2FA.

**Symptoms:**
- Login page hangs after entering username/password when WebAuthn is the configured 2FA method
- The site never shows a "try another method" fallback
- No browser prompt appears to tap the security key

**Tracking:** [WebKit Bug 205350](https://bugs.webkit.org/show_bug.cgi?id=205350) - "[WPE][GTK] Support WebAuthn"

**Workarounds:**
- Configure a non-WebAuthn 2FA method for accounts used in the app (TOTP, SMS, phone prompts)
- Log in via Firefox or Chrome first, then copy the session cookies, or use the app after the session is established (profile data persists across launches)
- For Google accounts: disable Advanced Protection Program or add a TOTP authenticator app as a backup method

**Scope:** This is a WebKitGTK limitation, not specific to this project. GNOME Web (Epiphany) and Tauri apps on Linux have the same issue.

## User Agent Override

WebKitGTK ignores wry's `with_user_agent()` for the JavaScript-visible `navigator.userAgent` property. The HTTP request header is set correctly, but JS code on the page still sees WebKitGTK's default UA string, which masquerades as Safari on macOS (since WebKit is the Safari engine).

This causes sites like WhatsApp Web to detect the browser as macOS Safari and show a download page instead of the web app.

**How it's handled:** When `user_agent` is set in the app config, please-make-me-an-app automatically injects a JS override via `Object.defineProperty(navigator, 'userAgent', ...)`. This patches the JS-visible property to match the configured UA string.

Some sites also check `navigator.vendor`, `navigator.platform`, or `window.chrome`. Use the `navigator.*` config fields to override these without inline JS. See `examples/whatsapp.yaml` for a working example.

## Ad Blocking

Ad blocking is implemented via JavaScript API interception (patching `fetch`, `XMLHttpRequest`, `Image`, `sendBeacon`, plus a `MutationObserver`). This blocks most ads and trackers but has limitations:

- `<script src="ad.js">` tags already in the HTML may execute before the blocker runs
- CSS `background-image` and `@import` requests cannot be intercepted from JS
- `<link rel="preload">` fetches are not blocked

For full content blocking, WebKitGTK's native `WebKitUserContentFilter` API would be needed, but wry does not expose it. See [docs/ad-blocking.md](ad-blocking.md) for the research.

## System Tray Event Loop

Tray menu events (Quit, Show/Hide) use a separate channel (`tray_icon::menu::MenuEvent::receiver()`) that is not part of tao's event system. With `ControlFlow::Wait`, the event loop blocks until a tao window event arrives, so tray menu clicks are never processed.

**How it's handled:** When the tray is enabled, the event loop uses `ControlFlow::WaitUntil` with a 250ms timeout instead of `ControlFlow::Wait`. This wakes the loop ~4 times per second to check the tray channel. When no tray is configured, the loop uses `Wait` with zero overhead.

## Popup Windows

All `window.open()` and `target="_blank"` requests are denied and opened in the system browser instead. This is intentional: wry/WebKitGTK's `NewWindowResponse::Allow` creates unmanaged GTK popup windows with no icon, navigation handler, tray integration, or CSS/JS injection, producing a broken UX for login flows, link clicks, and ads alike.

Google redirect URLs (`google.com/url?q=<encoded-url>`) are automatically unwrapped to extract the real destination before opening in the browser.

Combined with the JS-based ad blocker (`adblock: true` by default), this prevents most ad popup spam. For apps that auto-navigate to cross-domain URLs programmatically (e.g., banking sites with fraud-detection redirects), set `open_external_links: false` to silently drop external navigations.

## Maximize/Minimize Buttons Disabled on KDE Plasma

Without an explicit minimum window size, KDE Plasma may disable the maximize and minimize title bar buttons. This happens because KDE infers size constraints from the initial window dimensions when no minimum is set, treating the window as fixed-size.

**How it's handled:** The window is created with `with_min_inner_size(200x200)`, which tells the compositor the window is freely resizable. This enables maximize/minimize on KDE Plasma and other window managers.

## Always on Top

tao's `with_always_on_top()` and `set_always_on_top()` do not work reliably on KDE Plasma (Wayland). The window hint is either ignored by KWin or not propagated correctly through GTK to the compositor.

**Workaround:** Right-click the window title bar and use the compositor's built-in "Keep Above Others" option. This works on KDE Plasma and most other Linux desktop environments.

**Status:** Needs investigation. May require setting the X11 `_NET_WM_STATE_ABOVE` atom directly or using a KDE-specific D-Bus call on Wayland.

## Drag-and-Drop on Wayland

File drag-and-drop (e.g., dropping a file into WhatsApp Web to upload) may not work on Wayland. WebKitGTK passes an incorrect `time` parameter for drag-leave signals on Wayland, causing drop events to be misinterpreted as cancellations.

**Tracking:** [wry Issue #1256](https://github.com/tauri-apps/wry/issues/1256)

**Workaround:** Use X11 instead of Wayland, or use the file picker button in the web app.

## Cloudflare Turnstile Challenge

Sites protected by Cloudflare Turnstile (e.g., claude.ai, itch.io) fail the browser verification challenge in WebKitGTK. Turnstile fingerprints the browser engine, canvas rendering, WebGL, and other internals. Cloudflare's [supported browsers list](https://developers.cloudflare.com/cloudflare-challenges/reference/supported-browsers/) explicitly excludes embedded browsers and WebViews.

User agent spoofing does not help, and actually makes it worse: a Chrome UA paired with a WebKitGTK engine fingerprint is an extra-suspicious mismatch. Allowing `cloudflare.com` in `allowed_domains` does not help either (the challenge iframe loads fine, it's the verification that fails).

**Symptoms:**
- The Turnstile challenge loads but automatic verification fails
- Clicking "Verify you are human" loops back to the challenge
- Or a 403 JSON response: `{"error":{"type":"forbidden","message":"Request not allowed"}}`

**Affected sites:** claude.ai, itch.io, and any site using Cloudflare Turnstile or Bot Management in strict mode.

**Workaround:** None. These sites require a mainstream browser engine (Chromium, Firefox, or Safari on macOS). Use a regular browser instead.

**Scope:** This affects all WebKitGTK-based apps on Linux ([Tauri](https://github.com/tauri-apps/tauri/discussions/8524), [Lutris](https://github.com/lutris/lutris/issues/6215), GNOME Web). The macOS WebKit build (used by Safari) passes Turnstile because Cloudflare allowlists it.

## No Chrome Extension Support

WebKitGTK is not Chromium. Apps that depend on a Chrome extension for core functionality (e.g., Loom's screen recorder, Grammarly's inline editor) will not work. The extension APIs (`chrome.runtime`, `chrome.tabs`, manifest v3 service workers) do not exist in WebKitGTK.

Apps that use extensions only for optional features (e.g., a "share to X" button) will work fine without the extension, just with reduced functionality.

**Scope:** This is inherent to using a non-Chromium webview. Tauri and GNOME Web have the same limitation.

## Browser Mode Limitations

When `backend` is set to `brave`, `chrome`, or `chromium`, the app launches in the system browser's app mode (`--app`) instead of the embedded WebKitGTK webview. This is a fallback for sites that don't work with WebKitGTK (Cloudflare Turnstile, WebAuthn, etc.). Browser mode has its own trade-offs.

### What works in browser mode

- **Profile isolation** via `--user-data-dir` (full cookie, localStorage, and cache isolation per profile)
- **.desktop file generation** with icons and per-profile entries
- **Window size** from config (passed as `--window-size`)
- **Single instance** (Chromium enforces one instance per user-data-dir natively)
- **StartupWMClass** set in .desktop files for window/launcher matching

### What does not apply in browser mode

- **CSS/JS injection** (`inject.css`, `inject.js`, `inject.css_file`, `inject.js_file`): Not supported. Chromium removed the `--user-style-sheet` flag years ago and has no command-line flag for injecting scripts or stylesheets. Extensions like Tampermonkey can fill this gap manually. A future `--load-extension` approach is being considered (see [docs/ideas.md](../docs/ideas.md)).
- **User agent override** (`user_agent`): Ignored. The browser uses its own user agent string.
- **Ad blocking** (`adblock`): The JS-level blocker does not apply. Brave has built-in ad blocking. For Chrome/Chromium, install uBlock Origin or another extension.
- **Notification interception** (`notifications`): The config option is ignored. The browser handles web notifications natively with its own permission model.
- **System tray** (`tray`): Not supported. The browser manages its own window lifecycle.
- **Domain allow-listing** (`allowed_domains`): Ignored. The browser handles navigation natively; external links open in new tabs rather than being intercepted.
- **Clipboard control** (`clipboard`): Ignored. The browser manages clipboard access via its own permission model.
- **Window title** (`window.title`): In `--app` mode, Chromium uses the page's `<title>` tag.
- **Window position persistence** (`window.remember_position`): Ignored. Chromium remembers window position natively per user-data-dir.

### Data storage

Each backend stores data in its own subdirectory inside the profile, so switching backends does not cause conflicts:

```
$XDG_DATA_HOME/please-make-me-an-app/profiles/<app>/<profile>/
  webview-data/    -- WebKitGTK data (webview backend)
  chromium-data/   -- Chromium data (browser backend)
  lock             -- flock file (webview backend only)
  window_state.json
```

The `clear-cache` command removes the entire profile directory, including both backend data directories.

### Browser detection

| Backend    | Binary names searched in PATH         |
|------------|---------------------------------------|
| `brave`    | `brave-browser`, `brave`              |
| `chrome`   | `google-chrome`, `google-chrome-stable` |
| `chromium` | `chromium`, `chromium-browser`        |

If no matching binary is found, `open` fails with an error listing the searched names.
