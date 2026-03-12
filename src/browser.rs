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
pub fn build_args(config: &AppConfig, data_dir: &Path, profile_name: &str, url: &str) -> Vec<String> {
    let wm_class = if config.profiles.is_empty() {
        format!("pmma-{}", config.name)
    } else {
        format!("pmma-{}--{}", config.name, profile_name)
    };

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
        assert!(args.contains(&"--class=pmma-test-app".to_string()));
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
        assert!(args.contains(&"--class=pmma-test-app--work".to_string()));
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

}
