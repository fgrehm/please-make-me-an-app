# Ideas

Things that could be built but aren't planned for any specific release.

## Browser mode CSS/JS injection via --load-extension

**Implemented.** See `docs/known-limitations.md` (Browser Mode Limitations → CSS/JS injection).

## Devtools toggle

Add a config flag or `--devtools` CLI arg so users can open devtools in release builds. Useful for debugging custom CSS/JS injection. wry supports `with_devtools(true)` and `open_devtools()` (requires the `devtools` cargo feature).

## Cross-platform support

- macOS: WKWebView via wry, `.app` bundle generation
- Windows: WebView2 via wry, shortcut creation

## Binary packaging

Package individual apps as standalone binaries with the config embedded. Self-contained executables that don't require the CLI to be installed.

## Companion browser extension for domain-based routing

A browser extension (Chrome/Firefox) that intercepts HTTPS navigation and routes matching URLs to please-make-me-an-app instead. This would fill the gap described in [docs/known-limitations.md](known-limitations.md) under "No Domain-Specific HTTPS URL Handling."

**User-facing config** (sketch):

```yaml
# In the app config, declare which HTTPS origins this app should capture.
capture_origins:
  - https://www.notion.so
  - https://notion.so
```

**How it would work:**

1. The extension maintains a list of origins mapped to PMMA config file paths (synced from installed apps or configured manually).
2. On `webNavigation.onBeforeNavigate`, if the URL's origin matches, the extension:
   - Calls a native messaging host (`pmma-native-host`) bundled with the CLI, or
   - Redirects to a `pmma://open?config=<path>&url=<encoded-url>` URI handled by a registered scheme handler.
3. The native host or scheme handler invokes `please-make-me-an-app open <config> --url <url>`.
4. The browser tab is closed (or redirected to a blank page).

**Trade-offs:**
- Requires the extension to be installed in the user's browser.
- Native messaging requires a host manifest installed at the right path (`~/.config/google-chrome/NativeMessagingHosts/` etc.).
- The `pmma://` scheme approach is simpler to implement but passes the URL through the shell, which requires careful escaping.
- Firefox and Chrome both support native messaging; the extension could target both via WebExtension APIs.

## User-specified extensions for browser backend

Allow browser-mode apps to load user-specified Chrome extensions via `extensions: [/path/to/unpacked-ext]` in config. Useful for apps like Loom where the extension adds recording UI via content scripts.

Caveat: in `--app` mode the toolbar is hidden, so extensions that rely on a toolbar popup are unreachable. Extensions that inject content scripts still work. Auto-fetching CRX from Chrome Web Store is intentionally out of scope.
