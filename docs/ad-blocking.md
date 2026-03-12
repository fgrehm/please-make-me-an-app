# Research: Ad Blocking in WebKitGTK via wry

Date: 2026-03-05

## Question

Can we block ads and trackers in webview apps using wry's API?

## Short answer

No. wry 0.47 does not expose the WebKitGTK content filtering API. The only viable approaches are either limited (JS injection) or fragile (direct C FFI).

## What wry provides

| API | What it does | Useful for ad blocking? |
|-----|-------------|------------------------|
| `with_navigation_handler` | Intercepts top-level navigations | No. Ads load as subresources (images, scripts, iframes), not navigations. |
| `with_custom_protocol` | Registers custom URL schemes | No. Only handles schemes you register, not http/https. |
| `with_new_window_req_handler` | Intercepts popup windows | No. Only covers `window.open()` and `target="_blank"`. |
| `with_initialization_script` | Injects JS before page load | Partially. Can hide ad elements via CSS, but cannot prevent network requests. |

None of these intercept subresource HTTP requests, which is what ad blocking requires.

## What WebKitGTK provides (but wry doesn't expose)

WebKitGTK has a full content filtering API at the C level, using the same JSON rule format as Safari content blockers:

- **`WebKitUserContentFilterStore`** - compiles and stores JSON filter rules on disk
  - `webkit_user_content_filter_store_new(path)` - create a store
  - `webkit_user_content_filter_store_save(store, id, source, ...)` - compile and save rules
  - `webkit_user_content_filter_store_load(store, id, ...)` - load compiled rules

- **`WebKitUserContentManager`** - applies filters to a webview
  - `webkit_user_content_manager_add_filter(manager, filter)` - enable a filter
  - `webkit_user_content_manager_remove_all_filters(manager)` - clear filters

- **Rule format** - JSON array of trigger/action pairs:
  ```json
  [
    {
      "trigger": { "url-filter": ".*\\.doubleclick\\.net" },
      "action": { "type": "block" }
    }
  ]
  ```

This API works well and is how GNOME Web (Epiphany) implements its ad blocker. The problem is getting access to it from Rust through wry.

## Options considered

### 1. JavaScript-based element hiding (works today, limited)

Inject CSS via `with_initialization_script` to hide known ad elements:

```javascript
const style = document.createElement('style');
style.textContent = '.ad-banner, [id*="google_ads"] { display: none !important; }';
document.head.appendChild(style);
```

**Pros:** Works with current wry API. No extra dependencies.
**Cons:** Ads still load (bandwidth, tracking). Rules are CSS selectors only, not URL patterns. Limited effectiveness compared to real content blocking.

This is already possible via the existing `inject.css` config field.

### 2. Direct WebKitGTK C API via FFI (works, fragile)

Add `webkit2gtk-sys` as a dependency, get the underlying `WebKitWebView` pointer, and call the content filter C functions directly.

**Pros:** Full content blocking with standard filter lists.
**Cons:**
- Breaks wry's abstraction layer. Internal wry changes could invalidate our pointer access.
- wry's `WebViewExtUnix` trait doesn't expose the `WebKitWebView` pointer directly. Would need to traverse the GTK widget tree to find it.
- Async C API (filter compilation uses GLib async callbacks) is awkward from Rust.
- Linux-only. Would need separate implementations for macOS/Windows.
- Maintenance burden: must track both wry and WebKitGTK API changes.

### 3. Local filtering proxy (out of scope)

Run a local HTTP proxy that filters requests, configure WebKitGTK to use it.

**Pros:** Works with any webview engine.
**Cons:** Significant complexity. Breaks HTTPS without MITM cert setup. Overkill for this project.

### 4. Wait for wry support (ideal, timeline unknown)

Related wry issues:
- [#1087: How to intercept HTTP API requests](https://github.com/tauri-apps/wry/issues/1087)
- [#905: Only allow requests using custom protocol](https://github.com/tauri-apps/wry/issues/905)
- [#456: Intercept page redirects/navigation](https://github.com/tauri-apps/wry/issues/456)

The Tauri team is aware of the demand. No timeline for implementation.

## Recommendation

1. **Now:** Users who want ad hiding can use `inject.css` with element-hiding rules. Document this as a workaround.
2. **Later:** If wry adds content filter support or exposes the `WebKitWebView` pointer cleanly, implement proper blocking with EasyList/uBlock Origin filter lists.
3. **Alternative:** If ad blocking becomes critical, consider the direct FFI approach as a Linux-specific feature behind a cargo feature flag, accepting the maintenance cost.
