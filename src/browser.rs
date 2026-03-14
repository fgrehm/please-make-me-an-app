use crate::config::{AppConfig, Backend};
use anyhow::{bail, Context, Result};
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
pub fn build_args(config: &AppConfig, data_dir: &Path, _profile_name: &str, url: &str) -> Vec<String> {
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
pub fn run(config: &AppConfig, profile_name: &str, data_dir: &Path, url: &str) -> Result<()> {
    let binary = find_binary(&config.backend)?;
    let browser_data_dir = data_dir.join("chromium-data");
    let args = build_args(config, &browser_data_dir, profile_name, url);

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

    let (host_port, path) = match without_scheme.find('/') {
        Some(i) => (&without_scheme[..i], &without_scheme[i..]),
        None => (without_scheme, "/"),
    };

    // Strip port from host (e.g. "example.com:8080" -> "example.com")
    let host = match host_port.rfind(':') {
        Some(i) => &host_port[..i],
        None => host_port,
    };

    // Strip query string and fragment from path
    let path_only = path
        .find('?')
        .or_else(|| path.find('#'))
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

    if config.inject.has_content() {
        eprintln!(
            "warning: inject options are ignored with backend '{backend}' \
             (no CSS/JS injection support in browser mode)"
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
        let args = build_args(&config, Path::new("/data/test-app/default"), "default", "https://example.com");
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
        let args = build_args(&config, Path::new("/data/test-app/work"), "work", "https://example.com");
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
