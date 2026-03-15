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

## User-specified extensions for browser backend

Allow browser-mode apps to load user-specified Chrome extensions via `extensions: [/path/to/unpacked-ext]` in config. Useful for apps like Loom where the extension adds recording UI via content scripts.

Caveat: in `--app` mode the toolbar is hidden, so extensions that rely on a toolbar popup are unreachable. Extensions that inject content scripts still work. Auto-fetching CRX from Chrome Web Store is intentionally out of scope.
