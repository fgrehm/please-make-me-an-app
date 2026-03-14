# Copilot Code Review Instructions

## Project context

Rust CLI tool that turns websites into standalone desktop apps using native webviews (wry/tao on Linux). Single binary, no bundled browser engine. See CLAUDE.md for full architecture.

## What to focus on

- **Unsafe code**: only `libc::flock` should use `unsafe`. Flag any new `unsafe` blocks and verify safety comments are present.
- **Atomic ordering**: the event loop uses `AtomicBool` flags for cross-thread signaling. Stores must use `Release`, loads/swaps must use `Acquire`. Flag `Relaxed` on cross-thread atomics.
- **Error handling**: this is a binary, not a library. Use `anyhow` for errors. Flag custom error types unless justified. Flag `unwrap()` outside of tests (use `expect()` with a reason or `?`).
- **Platform-specific code**: Linux-only for now. `#[cfg(target_os = "linux")]` blocks must have a `#[cfg(not(target_os = "linux"))]` fallback that compiles (even if it's a no-op).
- **GTK/Wayland quirks**: check for proper trait imports (`GtkWindowExt`, `WidgetExt`) and that GTK operations go through the `gtk_window()` accessor, not tao's window methods directly.
- **Security**: no command injection in `std::process::Command` calls. No user input interpolated into JS strings without escaping. Check `escape_js_string()` is used where needed.
- **Dependencies**: flag new dependencies. This project minimizes its dependency tree. Prefer stdlib solutions.

## Accepted design decisions (do not flag these)

- **`process::exit(0)` for all exit paths**: `ControlFlow::Exit` waits for GTK cleanup and notification threads, causing noticeable shutdown delay. Window state is saved before exit. The flock is kernel-released on process exit. This is intentional.
- **Unconditional 250ms polling (`WaitUntil`)**: keyboard shortcuts, raise socket, and tray events use non-tao channels. `EventLoopProxy` cannot wake the loop from WebKitGTK IPC handlers. Documented in `docs/known-limitations.md`.
- **Per-notification `thread::spawn` for action listeners**: each notification click handler must be active immediately. Threads are bounded by the 10s notification timeout. Shutdown delay is avoided by `process::exit(0)`. A single sequential worker would miss clicks on overlapping notifications.
- **Image codecs (jpeg/gif/webp)**: real-world favicons are served in these formats. Silent decode failure without them breaks tray/window icons. Justified in `Cargo.toml` comment.
- **Browser backend WM_CLASS from URL**: Chromium ignores `--class` on Wayland and derives `app_id` from the URL. Per-profile distinction is impossible. Documented in `docs/known-limitations.md`.
- **IPC token authentication**: host-control IPC messages (quit, close, beforeunload) are prefixed with a per-launch nonce injected as `window.__pmma_token`. Validation happens in the IPC handler.

## What to ignore

- Clippy and formatting (handled by CI).
- Test coverage suggestions (tests are unit-level, no UI testing possible without a display server).
- Cross-platform portability comments (Linux-only is intentional, cross-platform is future work).
- Suggestions to replace `process::exit(0)` with `ControlFlow::Exit`.
- Suggestions to change the event loop polling strategy.
- Suggestions to change notification action listener threading model.
- Suggestions to reduce image codec features or make them optional.
