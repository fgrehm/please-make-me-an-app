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

## What to ignore

- Clippy and formatting (handled by CI).
- Test coverage suggestions (tests are unit-level, no UI testing possible without a display server).
- Cross-platform portability comments (Linux-only is intentional, cross-platform is future work).
