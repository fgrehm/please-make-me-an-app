use crate::config::{AppConfig, Backend};
use crate::inject;
use anyhow::{bail, Context, Result};
use serde_json::json;
use std::path::{Path, PathBuf};

/// Candidate binary names for each browser backend.
fn binary_candidates(backend: &Backend) -> &'static [&'static str] {
    match backend {
        Backend::Brave => &["brave-browser", "brave"],
        Backend::Chrome => &["google-chrome", "google-chrome-stable"],
        Backend::Chromium => &["chromium", "chromium-browser"],
        Backend::Webview => &[],
    }
}

/// Search PATH for an executable file with the given name.
fn find_in_path(name: &str) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|dir| {
            let full = dir.join(name);
            if let Ok(meta) = full.metadata() {
                if meta.is_file() && meta.permissions().mode() & 0o111 != 0 {
                    return Some(full);
                }
            }
            None
        })
    })
}

/// Find the browser binary for a given backend.
pub fn find_binary(backend: &Backend) -> Result<PathBuf> {
    let candidates = binary_candidates(backend);
    for name in candidates {
        if let Some(path) = find_in_path(name) {
            return Ok(path);
        }
    }
    bail!(
        "Could not find {} browser. Searched for: {}",
        backend.display_name(),
        candidates.join(", ")
    );
}

/// Build the command-line arguments for launching a Chromium-based browser in app mode.
pub fn build_args(config: &AppConfig, data_dir: &Path, url: &str) -> Vec<String> {
    // Use the Chromium-predicted WM class so StartupWMClass in the .desktop file
    // matches --class on X11. On Wayland, Chromium ignores --class entirely, but
    // passing the predicted value here keeps X11 consistent with the desktop file.
    let wm_class = chromium_wm_class(&config.backend, url);

    let mut args = vec![
        format!("--app={}", url),
        format!("--user-data-dir={}", data_dir.display()),
        format!("--class={}", wm_class),
        "--no-first-run".to_string(),
        "--no-default-browser-check".to_string(),
    ];

    if config.window.width > 0 && config.window.height > 0 {
        args.push(format!(
            "--window-size={},{}",
            config.window.width, config.window.height
        ));
    }

    args
}

/// Launch a Chromium-based browser in app mode and wait for it to exit.
pub fn run(config: &AppConfig, data_dir: &Path, url: &str, config_dir: &Path) -> Result<()> {
    let binary = find_binary(&config.backend)?;
    let browser_data_dir = data_dir.join("chromium-data");
    let mut args = build_args(config, &browser_data_dir, url);

    if let Some(ext_dir) = generate_extension(config, config_dir, data_dir)? {
        args.push(format!("--load-extension={}", ext_dir.display()));
    }

    let status = std::process::Command::new(&binary)
        .args(&args)
        .status()
        .with_context(|| format!("Failed to launch {}", binary.display()))?;

    if !status.success() {
        if let Some(code) = status.code() {
            bail!("{} exited with status {}", binary.display(), code);
        }
    }

    Ok(())
}

/// Auto-generate a minimal unpacked Chrome extension that injects the app's
/// CSS and JS via content scripts. Returns the extension directory path, or
/// None if the config has no inject content.
///
/// The extension is written to `<data_dir>/browser-extension/` (sibling of
/// `chromium-data/`) and passed to the browser via `--load-extension`.
pub fn generate_extension(
    config: &AppConfig,
    config_dir: &Path,
    data_dir: &Path,
) -> Result<Option<PathBuf>> {
    let css = inject::resolve_content(
        config.inject.css.as_deref(),
        config.inject.css_file.as_deref(),
        config_dir,
    )?;
    let js = inject::resolve_content(
        config.inject.js.as_deref(),
        config.inject.js_file.as_deref(),
        config_dir,
    )?;

    if css.is_none() && js.is_none() {
        return Ok(None);
    }

    let ext_dir = data_dir.join("browser-extension");
    std::fs::create_dir_all(&ext_dir)
        .with_context(|| format!("Failed to create extension dir: {}", ext_dir.display()))?;

    let match_pattern = url_origin_pattern(&config.url);
    let css_files: &[&str] = if css.is_some() { &["content.css"] } else { &[] };
    let js_files: &[&str] = if js.is_some() { &["content.js"] } else { &[] };

    if let Some(css) = css {
        std::fs::write(ext_dir.join("content.css"), css)
            .context("Failed to write extension content.css")?;
    }

    if let Some(js) = js {
        std::fs::write(ext_dir.join("content.js"), js)
            .context("Failed to write extension content.js")?;
    }

    let manifest = build_manifest(&config.name, &match_pattern, css_files, js_files);
    std::fs::write(ext_dir.join("manifest.json"), manifest)
        .context("Failed to write extension manifest.json")?;

    Ok(Some(ext_dir))
}

/// Build a Chrome extension match pattern covering the app's entire origin.
/// e.g. "https://mail.google.com/mail/u/0/" -> "https://mail.google.com/*"
fn url_origin_pattern(url: &str) -> String {
    let sep = url.find("://");
    let scheme = sep.map(|i| &url[..i + 3]).unwrap_or("https://");
    let rest = sep.map(|i| &url[i + 3..]).unwrap_or(url);
    let host = rest.split('/').next().unwrap_or(rest);
    format!("{}{}/*", scheme, host)
}

/// Build the manifest.json content for the unpacked extension.
fn build_manifest(name: &str, match_pattern: &str, css_files: &[&str], js_files: &[&str]) -> String {
    let mut script = json!({
        "matches": [match_pattern],
        "run_at": "document_start"
    });
    if !css_files.is_empty() {
        script["css"] = json!(css_files);
    }
    if !js_files.is_empty() {
        script["js"] = json!(js_files);
    }
    let manifest = json!({
        "manifest_version": 3,
        "name": format!("pmma-{}-inject", name),
        "version": "1",
        "content_scripts": [script]
    });
    serde_json::to_string_pretty(&manifest).expect("manifest serialization is infallible")
}

/// Replicate Chromium's `GenerateApplicationNameFromURL()`.
///
/// Given `"https://claude.ai/"`, produces `"claude.ai__"`.
/// The algorithm is: `(host + "_" + path).replace('/', '_')`.
/// Port, query string, and fragment are excluded (Chromium uses only host + path).
fn chromium_app_name_from_url(url: &str) -> String {
    let without_scheme = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);

    // Split authority from path at the first '/', '?', or '#'.
    // URLs like "example.com?x=1" (no explicit path slash) must not leak
    // query/fragment into host_port.
    let authority_end = without_scheme
        .find(['/', '?', '#'])
        .unwrap_or(without_scheme.len());
    let host_port = &without_scheme[..authority_end];
    let remainder = &without_scheme[authority_end..];
    let path = if remainder.is_empty() || !remainder.starts_with('/') {
        "/"
    } else {
        remainder
    };

    // Strip userinfo ("user:pass@host" -> "host") then strip port.
    // IPv6 literals are bracketed ("[::1]:3000"), so find the closing ']'
    // rather than the last ':', which would incorrectly split inside the address.
    let after_userinfo = host_port
        .find('@')
        .map(|i| &host_port[i + 1..])
        .unwrap_or(host_port);
    let host = if after_userinfo.starts_with('[') {
        after_userinfo.find(']').map(|i| &after_userinfo[..=i]).unwrap_or(after_userinfo)
    } else {
        after_userinfo.rfind(':').map(|i| &after_userinfo[..i]).unwrap_or(after_userinfo)
    };

    // Strip query string and fragment from path at whichever comes first
    let path_only = [path.find('?'), path.find('#')]
        .into_iter()
        .flatten()
        .min()
        .map(|i| &path[..i])
        .unwrap_or(path);

    format!("{}_{}", host, path_only).replace('/', "_")
}

/// The Wayland `app_id` that Chromium sets in `--app` mode.
///
/// Chromium ignores `--class` for app windows on Wayland and generates
/// its own `app_id` from the URL: `<browser>-<url_app_name>-Default`.
/// This must match `StartupWMClass` in the `.desktop` file for icon
/// matching to work.
pub fn chromium_wm_class(backend: &Backend, url: &str) -> String {
    let prefix = match backend {
        Backend::Brave => "brave",
        Backend::Chrome => "google-chrome",
        Backend::Chromium => "chromium-browser",
        Backend::Webview => unreachable!("chromium_wm_class called with webview backend"),
    };
    let app_name = chromium_app_name_from_url(url);
    format!("{}-{}-Default", prefix, app_name)
}

/// Print warnings for config options that are ignored in browser mode.
pub fn warn_ignored_options(config: &AppConfig) {
    let backend = config.backend.display_name();

    if config.inject.has_content() && matches!(config.backend, Backend::Chrome) {
        eprintln!(
            "warning: inject is supported via --load-extension with backend '{backend}', \
             but Chrome may show a developer-mode extensions notice on every launch"
        );
    }

    if config.user_agent.is_some() {
        eprintln!(
            "warning: user_agent is ignored with backend '{backend}' \
             (browser uses its own user agent)"
        );
    }

    if config.tray.enabled {
        eprintln!(
            "warning: tray is ignored with backend '{backend}' \
             (not supported in browser mode)"
        );
    }

    if !config.allowed_domains.is_empty() {
        eprintln!(
            "warning: allowed_domains is ignored with backend '{backend}' \
             (browser handles navigation natively)"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config;

    #[test]
    fn binary_candidates_brave() {
        let candidates = binary_candidates(&Backend::Brave);
        assert_eq!(candidates, &["brave-browser", "brave"]);
    }

    #[test]
    fn binary_candidates_chrome() {
        let candidates = binary_candidates(&Backend::Chrome);
        assert_eq!(candidates, &["google-chrome", "google-chrome-stable"]);
    }

    #[test]
    fn binary_candidates_chromium() {
        let candidates = binary_candidates(&Backend::Chromium);
        assert_eq!(candidates, &["chromium", "chromium-browser"]);
    }

    #[test]
    fn binary_candidates_webview_empty() {
        let candidates = binary_candidates(&Backend::Webview);
        assert!(candidates.is_empty());
    }

    #[test]
    fn build_args_basic() {
        let mut config = config::test_config();
        config.backend = Backend::Brave;
        let args = build_args(&config, Path::new("/data/test-app/default"), "https://example.com");
        assert!(args.contains(&"--app=https://example.com".to_string()));
        assert!(args.contains(&"--user-data-dir=/data/test-app/default".to_string()));
        // --class uses the Chromium-predicted WM class, not pmma-*
        assert!(args.contains(&"--class=brave-example.com__-Default".to_string()));
        assert!(args.contains(&"--no-first-run".to_string()));
        assert!(args.contains(&"--no-default-browser-check".to_string()));
        assert!(args.contains(&"--window-size=1200,800".to_string()));
    }

    #[test]
    fn build_args_with_profile() {
        let mut config = config::test_config();
        config.backend = Backend::Chrome;
        config.profiles = vec![
            config::ProfileConfig { name: "work".to_string() },
            config::ProfileConfig { name: "personal".to_string() },
        ];
        let args = build_args(&config, Path::new("/data/test-app/work"), "https://example.com");
        // Profile name does not affect --class; the WM class comes from the URL
        assert!(args.contains(&"--class=google-chrome-example.com__-Default".to_string()));
    }

    #[test]
    fn find_in_path_finds_existing_binary() {
        // "sh" should be on PATH in any Unix environment
        let result = find_in_path("sh");
        assert!(result.is_some());
    }

    #[test]
    fn find_in_path_returns_none_for_missing() {
        let result = find_in_path("this-binary-does-not-exist-pmma");
        assert!(result.is_none());
    }

    #[test]
    fn chromium_app_name_simple_host() {
        assert_eq!(chromium_app_name_from_url("https://claude.ai/"), "claude.ai__");
    }

    #[test]
    fn chromium_app_name_no_trailing_slash() {
        // URL without trailing slash gets implied /
        assert_eq!(chromium_app_name_from_url("https://claude.ai"), "claude.ai__");
    }

    #[test]
    fn chromium_app_name_with_path() {
        assert_eq!(
            chromium_app_name_from_url("https://mail.google.com/mail/u/0/"),
            "mail.google.com__mail_u_0_"
        );
    }

    #[test]
    fn chromium_app_name_strips_port() {
        assert_eq!(
            chromium_app_name_from_url("https://localhost:3000/app"),
            "localhost__app"
        );
    }

    #[test]
    fn chromium_app_name_strips_query() {
        assert_eq!(
            chromium_app_name_from_url("https://example.com/path?foo=bar"),
            "example.com__path"
        );
    }

    #[test]
    fn chromium_app_name_strips_fragment() {
        assert_eq!(
            chromium_app_name_from_url("https://example.com/path#section"),
            "example.com__path"
        );
    }

    #[test]
    fn chromium_app_name_query_without_path() {
        // URL like "https://example.com?x=1" (no path slash before query)
        assert_eq!(
            chromium_app_name_from_url("https://example.com?x=1"),
            "example.com__"
        );
    }

    #[test]
    fn chromium_app_name_fragment_without_path() {
        // URL like "https://example.com#frag" (no path slash before fragment)
        assert_eq!(
            chromium_app_name_from_url("https://example.com#frag"),
            "example.com__"
        );
    }

    #[test]
    fn chromium_app_name_strips_userinfo() {
        // userinfo must be stripped so rfind(':') doesn't split inside "user:pass"
        assert_eq!(
            chromium_app_name_from_url("https://user:pass@example.com/app"),
            "example.com__app"
        );
    }

    #[test]
    fn chromium_app_name_ipv6_no_port() {
        // rfind(':') would incorrectly split inside "[::1]" without the bracket check
        assert_eq!(
            chromium_app_name_from_url("https://[::1]/path"),
            "[::1]__path"
        );
    }

    #[test]
    fn chromium_app_name_ipv6_with_port() {
        assert_eq!(
            chromium_app_name_from_url("https://[::1]:3000/path"),
            "[::1]__path"
        );
    }

    #[test]
    fn url_origin_pattern_strips_path() {
        assert_eq!(
            url_origin_pattern("https://mail.google.com/mail/u/0/"),
            "https://mail.google.com/*"
        );
    }

    #[test]
    fn url_origin_pattern_bare_host() {
        assert_eq!(url_origin_pattern("https://example.com"), "https://example.com/*");
    }

    #[test]
    fn url_origin_pattern_preserves_scheme() {
        assert_eq!(
            url_origin_pattern("http://localhost:3000/app"),
            "http://localhost:3000/*"
        );
    }

    #[test]
    fn build_manifest_with_css_and_js() {
        let m = build_manifest("myapp", "https://example.com/*", &["content.css"], &["content.js"]);
        let v: serde_json::Value = serde_json::from_str(&m).unwrap();
        assert_eq!(v["manifest_version"], 3);
        assert_eq!(v["name"], "pmma-myapp-inject");
        let script = &v["content_scripts"][0];
        assert_eq!(script["matches"][0], "https://example.com/*");
        assert_eq!(script["css"][0], "content.css");
        assert_eq!(script["js"][0], "content.js");
        assert_eq!(script["run_at"], "document_start");
    }

    #[test]
    fn build_manifest_omits_js_key_when_css_only() {
        let m = build_manifest("app", "https://example.com/*", &["content.css"], &[]);
        let v: serde_json::Value = serde_json::from_str(&m).unwrap();
        let script = &v["content_scripts"][0];
        assert_eq!(script["css"][0], "content.css");
        assert!(script["js"].is_null());
    }

    #[test]
    fn build_manifest_omits_css_key_when_js_only() {
        let m = build_manifest("app", "https://example.com/*", &[], &["content.js"]);
        let v: serde_json::Value = serde_json::from_str(&m).unwrap();
        let script = &v["content_scripts"][0];
        assert!(script["css"].is_null());
        assert_eq!(script["js"][0], "content.js");
    }

    #[test]
    fn generate_extension_no_content_returns_none() {
        let config = config::test_config();
        let dir = tempfile::tempdir().unwrap();
        let result = generate_extension(&config, dir.path(), dir.path()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn generate_extension_writes_css_file_for_inline_css() {
        let mut config = config::test_config();
        config.inject.css = Some(".ad { display: none; }".to_string());
        let dir = tempfile::tempdir().unwrap();
        let ext_dir = generate_extension(&config, dir.path(), dir.path()).unwrap().unwrap();
        let css = std::fs::read_to_string(ext_dir.join("content.css")).unwrap();
        assert!(css.contains(".ad { display: none; }"));
        assert!(ext_dir.join("manifest.json").exists());
    }

    #[test]
    fn generate_extension_skips_js_file_when_no_js_config() {
        let mut config = config::test_config();
        config.inject.css = Some(".ad { display: none; }".to_string());
        let dir = tempfile::tempdir().unwrap();
        let ext_dir = generate_extension(&config, dir.path(), dir.path()).unwrap().unwrap();
        assert!(!ext_dir.join("content.js").exists());
    }

    #[test]
    fn generate_extension_writes_js_file_for_inline_js() {
        let mut config = config::test_config();
        config.inject.js = Some("console.log('hello');".to_string());
        let dir = tempfile::tempdir().unwrap();
        let ext_dir = generate_extension(&config, dir.path(), dir.path()).unwrap().unwrap();
        assert!(ext_dir.join("manifest.json").exists());
        assert!(ext_dir.join("content.js").exists());
        assert!(!ext_dir.join("content.css").exists());
    }

    #[test]
    fn generate_extension_manifest_uses_app_url_origin() {
        let mut config = config::test_config();
        config.url = "https://mail.google.com/mail/u/0/".to_string();
        config.inject.js = Some("console.log('hi');".to_string());
        let dir = tempfile::tempdir().unwrap();
        let ext_dir = generate_extension(&config, dir.path(), dir.path()).unwrap().unwrap();
        let manifest: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(ext_dir.join("manifest.json")).unwrap())
                .unwrap();
        assert_eq!(
            manifest["content_scripts"][0]["matches"][0],
            "https://mail.google.com/*"
        );
    }

    #[test]
    fn chromium_wm_class_brave() {
        assert_eq!(
            chromium_wm_class(&Backend::Brave, "https://claude.ai/"),
            "brave-claude.ai__-Default"
        );
    }

    #[test]
    fn chromium_wm_class_chrome() {
        assert_eq!(
            chromium_wm_class(&Backend::Chrome, "https://mail.google.com/mail/"),
            "google-chrome-mail.google.com__mail_-Default"
        );
    }

    #[test]
    fn chromium_wm_class_chromium() {
        assert_eq!(
            chromium_wm_class(&Backend::Chromium, "https://example.com"),
            "chromium-browser-example.com__-Default"
        );
    }
}
