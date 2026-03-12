# Rust Basics for Reviewing This Codebase

A quick reference for the Rust patterns used in please-make-me-an-app. Not a Rust tutorial, just enough to read and review the code confidently.

## Ownership and borrowing

Rust's core idea: every value has exactly one owner. When the owner goes out of scope, the value is dropped (freed). You can temporarily lend access via references.

```rust
let name = String::from("whatsapp");  // `name` owns the string
let r = &name;                         // `r` borrows it (read-only)
let m = &mut name;                     // `m` borrows it (read-write, exclusive)
```

You'll see `&str` vs `String` throughout the code:
- `String` = owned, heap-allocated, growable string. You can modify it.
- `&str` = borrowed reference to string data. Cheap to pass around. Can't modify.
- `"hello"` literals are `&str` (they live in the binary).
- `.as_str()`, `.as_deref()` convert `String`/`Option<String>` to `&str`/`Option<&str>`.
- `.to_string()` converts `&str` to an owned `String`.

Similar pattern with paths:
- `PathBuf` = owned path (like `String`).
- `&Path` = borrowed path (like `&str`).

## Common types

### Option and Result

```rust
Option<T>      // either Some(value) or None. Rust's null replacement.
Result<T, E>   // either Ok(value) or Err(error). Used for fallible operations.
```

Pattern matching to handle them:
```rust
match some_option {
    Some(val) => use(val),
    None => handle_missing(),
}

// Shorthand: `if let` for when you only care about one variant
if let Some(ua) = &config.user_agent {
    builder = builder.with_user_agent(ua);
}

// The ? operator: return early if Err/None, unwrap if Ok/Some
let contents = std::fs::read_to_string(path)?;  // returns Err if file read fails
```

### Vec, HashSet

```rust
Vec<T>         // growable array (like ArrayList/list)
HashSet<T>     // unique set (like Set)
&[T]           // borrowed slice of a Vec/array. Read-only view.
```

## Structs and derive macros

```rust
#[derive(Debug, Deserialize)]    // auto-generate Debug printing and YAML/JSON parsing
pub struct AppConfig {
    pub name: String,            // `pub` = visible outside this module
    #[serde(default)]            // use Default::default() if field missing in YAML
    pub clipboard: bool,
    #[serde(default = "default_width")]  // call this function for the default
    pub width: u32,
}
```

`#[derive(...)]` generates code automatically:
- `Debug` = can print with `{:?}` format
- `Default` = has a `::default()` that returns zero/empty/false values
- `Deserialize` = serde can parse it from YAML/JSON
- `Clone` = can call `.clone()` to make a copy

## Error handling with anyhow

This project uses `anyhow` for error handling, which is the standard choice for applications (as opposed to libraries).

```rust
use anyhow::{bail, Context, Result};

fn load(path: &Path) -> Result<AppConfig> {     // Result<T> = anyhow::Result<T, anyhow::Error>
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read: {}", path.display()))?;
    //  ^^^^^^^^^^^^^^ adds context message if the operation fails
    //                                                               ^ early return on error

    if text.is_empty() {
        bail!("Config file is empty");   // bail! = return Err(...) with a formatted message
    }

    Ok(config)
}
```

The `?` operator is the key pattern. It means "if this is an Err, return it from the current function; if Ok, unwrap the value." Almost every function that can fail returns `Result<T>`.

## Closures

```rust
// Short closures (like arrow functions / lambdas)
let doubled: Vec<_> = numbers.iter().map(|n| n * 2).collect();

// Move closures: take ownership of captured variables
let name = config.name.clone();
builder = builder.with_ipc_handler(move |req| {
    // `move` means this closure owns `name`, not just borrows it.
    // Needed when the closure outlives the current scope.
    println!("{}", name);
});
```

You'll see `move` closures in app.rs where callbacks are passed to wry/tao. The event loop outlives the function that creates it, so closures must own their data.

## Modules and visibility

```rust
// main.rs declares modules
mod config;      // loads src/config.rs
mod app;         // loads src/app.rs

// Using items from other modules
use crate::config::AppConfig;   // `crate` = root of this project
use anyhow::Result;              // from external crate
use std::path::Path;             // from standard library
```

- `pub` = public, visible outside the module
- no `pub` = private, only visible within the module
- `pub(crate)` = visible within the crate but not to external users

## Testing

```rust
#[cfg(test)]          // only compile this block when running tests
mod tests {
    use super::*;     // import everything from the parent module

    #[test]
    fn my_test() {
        assert!(true);
        assert_eq!(1 + 1, 2);
        assert!(result.is_ok());
        assert!(result.is_err());
    }
}
```

Tests live in the same file as the code they test, inside a `#[cfg(test)]` module. Run with `cargo test`.

The `#[cfg(test)]` block must be the **last item** in the file. Clippy enforces this (`items_after_test_module`).

### Shared test helpers

To avoid duplicating test setup across modules, `config.rs` exports a `test_config()` function that returns a minimal valid `AppConfig`. Other modules use it:

```rust
// In profile.rs tests
fn config_with_profiles(profiles: Vec<&str>) -> AppConfig {
    let mut config = crate::config::test_config();
    config.profiles = profiles.into_iter().map(|n| ProfileConfig { name: n.to_string() }).collect();
    config
}
```

The function is `pub` but `#[cfg(test)]`, so it only exists in test builds.

## Traits (interfaces)

Traits are Rust's version of interfaces. You'll see them used implicitly through derive macros, but occasionally explicitly:

```rust
// Display trait = how to print with {}
// Debug trait = how to print with {:?}
// Deserialize trait = how to parse from YAML/JSON
// Default trait = how to create a zero/empty value
```

The `impl` keyword implements functionality for a type:
```rust
impl Default for WindowConfig {
    fn default() -> Self {
        Self { title: "please-make-me-an-app".to_string(), width: 1200, height: 800 }
    }
}
```

## Iterators and method chains

Rust uses method chains on iterators instead of for loops for data transformation:

```rust
let domains: Vec<&str> = blocklist
    .lines()                            // split into lines (iterator)
    .map(|l| l.trim())                  // trim whitespace from each
    .filter(|l| !l.is_empty())          // keep non-empty
    .collect();                         // gather into a Vec
```

Common iterator methods: `.map()`, `.filter()`, `.any()`, `.find()`, `.collect()`, `.join()`.

## String formatting

```rust
format!("hello {}", name)              // like printf / template literals
format!("{:?}", config)                // debug print (shows struct internals)
eprintln!("error: {}", msg)            // print to stderr
println!("info: {}", msg)             // print to stdout
```

The `{}` placeholder uses the Display trait, `{:?}` uses Debug.

## Pattern matching

Beyond simple `match`, Rust has destructuring patterns:

```rust
// Match on enum variants with fields
if let Event::WindowEvent {
    event: WindowEvent::CloseRequested,
    ..                                    // ignore other fields
} = event {
    // handle close
}

// Match with guards
match input.parse::<usize>() {
    Ok(n) if n >= 1 && n <= max => Ok(profiles[n - 1].clone()),
    _ => bail!("Invalid selection"),      // _ matches anything
}
```

## Lifetimes (briefly)

You'll occasionally see `'a` annotations:
```rust
fn extract_domain(url: &str) -> Option<&str>
```

This means the returned `&str` borrows from the input `url`. The compiler infers this automatically here. You rarely need to write lifetime annotations in application code.

## Platform-specific code

```rust
#[cfg(target_os = "linux")]       // only compile on Linux
{
    // Linux-specific code
}

#[cfg(not(target_os = "linux"))]  // compile on everything except Linux
{
    // fallback
}
```

Used in app.rs for the WebKitGTK build path vs generic wry build, and in the browser-opening function for xdg-open vs open vs cmd.

## Macros

Macros end with `!` and generate code at compile time:

```rust
include_str!("../data/file.txt")   // embed file contents as &str at compile time
format!("hello {}", x)             // string formatting
vec![1, 2, 3]                      // create a Vec
println!(), eprintln!()            // print to stdout/stderr
assert!(), assert_eq!()            // test assertions
bail!("error message")             // return Err (from anyhow)
```

## Clippy and linting

Clippy is Rust's built-in linter. This project configures lint levels in `Cargo.toml`:

```toml
[lints.rust]
future-incompatible = "warn"
nonstandard-style = "deny"

[lints.clippy]
all = { level = "deny", priority = -1 }
redundant_clone = "deny"
needless_collect = "warn"
large_enum_variant = "warn"
```

Run it with:
```sh
cargo clippy --all-targets
```

Key things clippy catches:
- Unnecessary `.clone()` calls (use `&T` instead)
- Collecting into a `Vec` just to iterate again
- Enum variants that are much larger than others (box the big one)

If clippy complains but you know the code is correct, use `#[expect(clippy::lint_name)]` (not `#[allow(...)]`) with a comment explaining why. `#[expect]` warns you when the suppression becomes unnecessary (the lint stops firing), so stale suppressions don't accumulate. `#[allow]` stays forever and silently hides the problem even after the underlying code changes.

## Safety comments

Every `unsafe` block must have a `// SAFETY:` comment explaining why the operation is sound:

```rust
// SAFETY: `file` is a valid, open File and `as_raw_fd()` returns a valid descriptor.
// flock() is safe to call on any valid fd; it only manipulates advisory locks.
let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
```

The only `unsafe` in this codebase is the `flock` call for single-instance locking in `profile.rs`.

## `unwrap` vs `expect`

Both `unwrap()` and `expect()` panic if the value is `None`/`Err`. Prefer `expect()` with a message explaining *why* the value is always `Some`/`Ok` at that point:

```rust
// Bad: gives no context on panic
let vbox = window.default_vbox().unwrap();

// Good: explains the invariant that makes this safe
let vbox = window
    .default_vbox()
    .expect("GTK windows always have a default vbox on Linux");
```

Never use `unwrap()` in production code unless failure is truly impossible and a comment says so.

## Extracting helpers to remove duplication

When two functions share a repeated expression (like a string escaping pattern), extract it:

```rust
// Before: same expression in two places
fn build_ua_script(ua: &str) -> String {
    let escaped = ua.replace('\\', "\\\\").replace('\'', "\\'");
    // ...
}
fn build_nav_script(val: &str) -> String {
    let escaped = val.replace('\\', "\\\\").replace('\'', "\\'");  // duplicate
    // ...
}

// After: one private helper
fn escape_js_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\'', "\\'")
}
fn build_ua_script(ua: &str) -> String {
    format!("... '{}' ...", escape_js_string(ua))
}
fn build_nav_script(val: &str) -> String {
    format!("... '{}' ...", escape_js_string(val))
}
```

## When comments help vs. hurt

Comments should explain **why**, not **what**. If the code is clear, skip the comment.

Good:
```rust
// WebKitGTK ignores wry's with_user_agent for the JS-visible property,
// so we patch it via Object.defineProperty.
builder = builder.with_initialization_script(build_ua_override_script(ua));
```

Bad:
```rust
// Ad/tracker blocking via JS interception
if config.adblock { ... }
```

The second one restates the code. The first one explains a non-obvious workaround.

## `&'static str` vs `String` for constants

If a function returns a string that's known at compile time (a literal or built from `concat!`), return `&'static str` instead of `String`. This avoids a heap allocation every time the function is called.

```rust
// Good: the string literal lives in the binary, no allocation needed
pub fn intercept_script() -> &'static str {
    r#"(function() { ... })();"#
}

// Unnecessary: allocates a new String each call for a compile-time constant
pub fn intercept_script() -> String {
    r#"(function() { ... })();"#.to_string()
}
```

`&'static str` works anywhere a `&str` is expected, so callers don't need to change. If a caller needs an owned `String`, they can call `.to_string()` at the call site.

## Common patterns in this codebase

### Builder pattern
wry and tao use builders extensively:
```rust
let window = WindowBuilder::new()
    .with_title("My App")
    .with_inner_size(LogicalSize::new(1200, 800))
    .build(&event_loop)?;
```
Each `.with_*()` call returns `self`, allowing method chaining. `.build()` consumes the builder and produces the final object.

### Clone for closure ownership
When passing data into `move` closures, you often need to clone first:
```rust
let app_domain = extract_domain(&config.url).to_string();
let allowed = config.allowed_domains.clone();

// Each move closure needs its own copy (two closures, two clones)
let nav_domain = app_domain.clone();
let nav_allowed = allowed.clone();
builder = builder.with_navigation_handler(move |url| {
    // nav_domain and nav_allowed moved into here
});

builder = builder.with_new_window_req_handler(move |url, _| {
    // app_domain and allowed moved into here
});
```
Each `move` closure takes ownership, so if two closures need the same data, you clone it. This is one of the few places where cloning is unavoidable. Always leave a comment explaining why.

### The `let _ = expr` pattern
```rust
let _ = &_webview;   // intentionally keep _webview alive without using it
let _ = command.spawn();  // ignore the Result (we don't care if it fails)
```

### Conditional building
```rust
let mut builder = WebViewBuilder::new();
if condition {
    builder = builder.with_something(value);  // reassign to add optional config
}
```
Since builder methods consume `self` and return a new builder, you reassign to the same variable.
