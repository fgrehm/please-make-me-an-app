use anyhow::{bail, Context, Result};
use directories::ProjectDirs;
use serde::Deserialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub const APP_NAME: &str = "please-make-me-an-app";
pub const DEFAULT_PROFILE: &str = "default";

pub fn project_dirs() -> Result<ProjectDirs> {
    ProjectDirs::from("", "", APP_NAME).context("Failed to determine XDG directories")
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
#[clap(rename_all = "lowercase")]
pub enum Backend {
    #[default]
    Webview,
    Brave,
    Chrome,
    Chromium,
}

impl Backend {
    pub fn is_browser(&self) -> bool {
        !matches!(self, Backend::Webview)
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Backend::Webview => "webview",
            Backend::Brave => "brave",
            Backend::Chrome => "chrome",
            Backend::Chromium => "chromium",
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct AppConfig {
    pub name: String,
    pub url: String,

    #[serde(default)]
    pub backend: Backend,

    #[serde(default)]
    pub window: WindowConfig,

    #[serde(default)]
    pub profiles: Vec<ProfileConfig>,

    #[serde(default)]
    pub inject: InjectConfig,

    #[serde(default)]
    pub user_agent: Option<String>,

    #[serde(default)]
    pub navigator: NavigatorConfig,

    #[serde(default = "default_clipboard")]
    pub clipboard: bool,

    #[serde(default)]
    pub allowed_domains: Vec<String>,

    #[serde(default)]
    pub excluded_domains: Vec<String>,

    #[serde(default = "default_open_external_links")]
    pub open_external_links: bool,

    #[serde(default = "default_adblock")]
    pub adblock: bool,

    #[serde(default)]
    pub adblock_extra: Option<std::path::PathBuf>,

    #[serde(default = "default_notifications")]
    pub notifications: bool,

    #[serde(default)]
    pub tray: TrayConfig,

    #[serde(default)]
    pub url_schemes: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct NavigatorConfig {
    #[serde(default)]
    pub vendor: Option<String>,

    #[serde(default)]
    pub platform: Option<String>,

    #[serde(default)]
    pub chrome: bool,
}

#[derive(Debug, Deserialize)]
pub struct WindowConfig {
    #[serde(default = "default_title")]
    pub title: String,

    #[serde(default = "default_width")]
    pub width: u32,

    #[serde(default = "default_height")]
    pub height: u32,

    #[serde(default)]
    pub remember_position: bool,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            title: default_title(),
            width: default_width(),
            height: default_height(),
            remember_position: false,
        }
    }
}

fn default_title() -> String {
    "please-make-me-an-app".to_string()
}

fn default_width() -> u32 {
    1200
}

fn default_height() -> u32 {
    800
}

fn default_clipboard() -> bool {
    true
}

fn default_adblock() -> bool {
    true
}

fn default_open_external_links() -> bool {
    true
}

fn default_notifications() -> bool {
    true
}

#[derive(Debug, Default, Deserialize)]
pub struct TrayConfig {
    #[serde(default)]
    pub enabled: bool,

    #[serde(default)]
    pub minimize_to_tray: bool,
}

#[derive(Debug, Deserialize)]
pub struct ProfileConfig {
    pub name: String,
}

#[derive(Debug, Default, Deserialize)]
pub struct InjectConfig {
    #[serde(default)]
    pub css: Option<String>,

    #[serde(default)]
    pub js: Option<String>,

    #[serde(default)]
    pub css_file: Option<PathBuf>,

    #[serde(default)]
    pub js_file: Option<PathBuf>,
}

impl InjectConfig {
    pub fn has_content(&self) -> bool {
        self.css.is_some()
            || self.js.is_some()
            || self.css_file.is_some()
            || self.js_file.is_some()
    }
}

pub fn load(path: &Path) -> Result<AppConfig> {
    let app_yaml = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file: {}", path.display()))?;

    let config: AppConfig = match load_global_defaults()? {
        Some(defaults_yaml) => {
            let base: serde_yaml_ng::Value = serde_yaml_ng::from_str(&defaults_yaml)
                .context("Failed to parse global defaults file")?;
            let over: serde_yaml_ng::Value = serde_yaml_ng::from_str(&app_yaml)
                .with_context(|| format!("Failed to parse config file: {}", path.display()))?;
            let merged = merge_yaml(base, over);
            serde_yaml_ng::from_value(merged)
                .with_context(|| format!("Failed to parse config file: {}", path.display()))?
        }
        None => serde_yaml_ng::from_str(&app_yaml)
            .with_context(|| format!("Failed to parse config file: {}", path.display()))?,
    };

    validate(&config)?;

    Ok(config)
}

/// Build a config for an ad-hoc URL with no on-disk config file.
///
/// Name and window title are derived from the URL's host. Global defaults
/// (defaults.yaml) are merged in so user-wide settings (adblock, clipboard,
/// notifications) still apply.
pub fn ad_hoc(url: &str, backend: Backend) -> Result<(AppConfig, String)> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        bail!("URL '{}' must start with http:// or https://", url);
    }
    let host = url_host(url).unwrap_or("ad-hoc");
    let name = sanitize_name(host);
    let title = host.to_string();

    let synthesized = format!(
        "name: {}\nurl: {}\nbackend: {}\nwindow:\n  title: {}\n",
        name,
        url,
        backend.display_name(),
        // YAML scalar: quote in case the host contains a colon (e.g. localhost:3000).
        serde_yaml_ng::to_string(&title)?.trim()
    );

    let config: AppConfig = match load_global_defaults()? {
        Some(defaults_yaml) => {
            let base: serde_yaml_ng::Value = serde_yaml_ng::from_str(&defaults_yaml)
                .context("Failed to parse global defaults file")?;
            let over: serde_yaml_ng::Value = serde_yaml_ng::from_str(&synthesized)
                .context("Failed to parse synthesized ad-hoc config")?;
            let merged = merge_yaml(base, over);
            serde_yaml_ng::from_value(merged)
                .context("Failed to materialize ad-hoc config after defaults merge")?
        }
        None => serde_yaml_ng::from_str(&synthesized)
            .context("Failed to parse synthesized ad-hoc config")?,
    };

    validate(&config)?;
    Ok((config, name))
}

/// Extract the host portion of an http(s) URL.
fn url_host(url: &str) -> Option<&str> {
    let rest = url.strip_prefix("http://").or_else(|| url.strip_prefix("https://"))?;
    let host_with_port = rest.split('/').next()?;
    let host = host_with_port.split('?').next()?.split('#').next()?;
    if host.is_empty() {
        None
    } else {
        Some(host)
    }
}

/// Reduce a host string to the alphanumeric/hyphen/underscore set required by
/// `is_valid_name`. Dots and colons become hyphens; runs of separators collapse
/// so the result never has consecutive hyphens.
fn sanitize_name(host: &str) -> String {
    let mut out = String::with_capacity(host.len());
    let mut last_was_sep = false;
    for c in host.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            out.push(c.to_ascii_lowercase());
            last_was_sep = false;
        } else if !last_was_sep {
            out.push('-');
            last_was_sep = true;
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "ad-hoc".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Load global defaults from $XDG_CONFIG_HOME/please-make-me-an-app/defaults.yaml.
/// Returns Ok(None) if the file doesn't exist.
fn load_global_defaults() -> Result<Option<String>> {
    let dirs = project_dirs()?;
    let path = dirs.config_dir().join("defaults.yaml");
    match std::fs::read_to_string(&path) {
        Ok(contents) => Ok(Some(contents)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => {
            Err(e).with_context(|| format!("Failed to read global defaults: {}", path.display()))
        }
    }
}

/// Deep merge two YAML values. Per-app (over) wins for scalar fields.
/// Mappings are merged recursively so global defaults fill in gaps.
fn merge_yaml(base: serde_yaml_ng::Value, over: serde_yaml_ng::Value) -> serde_yaml_ng::Value {
    use serde_yaml_ng::Value;
    match (base, over) {
        (Value::Mapping(mut base_map), Value::Mapping(over_map)) => {
            for (key, over_val) in over_map {
                let merged = match base_map.remove(&key) {
                    Some(base_val) => merge_yaml(base_val, over_val),
                    None => over_val,
                };
                base_map.insert(key, merged);
            }
            Value::Mapping(base_map)
        }
        (_, over) => over,
    }
}

fn is_valid_scheme(s: &str) -> bool {
    // RFC 3986: scheme = ALPHA *( ALPHA / DIGIT / "+" / "-" / "." )
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.')
}

fn is_valid_name(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
}

fn validate(config: &AppConfig) -> Result<()> {
    if config.name.is_empty() {
        bail!("App name cannot be empty. Add a 'name' field to your config.");
    }
    if !is_valid_name(&config.name) {
        bail!(
            "App name '{}' contains invalid characters. \
             Use only letters, numbers, hyphens, and underscores.",
            config.name
        );
    }
    if config.name.contains("--") {
        bail!(
            "App name '{}' cannot contain consecutive hyphens.",
            config.name
        );
    }

    if !config.url.starts_with("http://") && !config.url.starts_with("https://") {
        bail!(
            "URL '{}' is not valid. URLs must start with http:// or https://",
            config.url
        );
    }

    if config.window.width == 0 || config.window.height == 0 {
        bail!("Window dimensions must be greater than zero.");
    }
    if config.window.width > 10000 || config.window.height > 10000 {
        bail!(
            "Window dimensions {}x{} are unreasonably large (max 10000).",
            config.window.width,
            config.window.height
        );
    }

    for scheme in &config.url_schemes {
        if !is_valid_scheme(scheme) {
            bail!(
                "Invalid URL scheme '{}'. Must start with a letter and contain only \
                 letters, digits, '+', '-', or '.'.",
                scheme
            );
        }
    }

    let mut profile_names = HashSet::new();
    for profile in &config.profiles {
        if profile.name.is_empty() {
            bail!("Profile names cannot be empty.");
        }
        if !is_valid_name(&profile.name) {
            bail!(
                "Profile name '{}' contains invalid characters. \
                 Use only letters, numbers, hyphens, and underscores.",
                profile.name
            );
        }
        if profile.name.contains("--") {
            bail!(
                "Profile name '{}' cannot contain consecutive hyphens.",
                profile.name
            );
        }
        if !profile_names.insert(&profile.name) {
            bail!("Duplicate profile name: '{}'", profile.name);
        }
    }

    Ok(())
}

/// Test helper: returns a minimal valid AppConfig for use in tests.
#[cfg(test)]
pub fn test_config() -> AppConfig {
    AppConfig {
        name: "test-app".to_string(),
        url: "https://example.com".to_string(),
        backend: Backend::default(),
        window: WindowConfig::default(),
        profiles: vec![],
        inject: InjectConfig::default(),
        user_agent: None,
        clipboard: true,
        allowed_domains: vec![],
        excluded_domains: vec![],
        open_external_links: true,
        navigator: NavigatorConfig::default(),
        adblock: true,
        adblock_extra: None,
        notifications: true,
        tray: TrayConfig::default(),
        url_schemes: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- validation tests --

    #[test]
    fn valid_config_passes() {
        assert!(validate(&test_config()).is_ok());
    }

    #[test]
    fn empty_name_rejected() {
        let mut config = test_config();
        config.name = "".to_string();
        let err = validate(&config).unwrap_err().to_string();
        assert!(err.contains("name cannot be empty"));
    }

    #[test]
    fn name_with_spaces_rejected() {
        let mut config = test_config();
        config.name = "my app".to_string();
        let err = validate(&config).unwrap_err().to_string();
        assert!(err.contains("invalid characters"));
    }

    #[test]
    fn name_with_hyphens_and_underscores_accepted() {
        let mut config = test_config();
        config.name = "my-app_v2".to_string();
        assert!(validate(&config).is_ok());
    }

    #[test]
    fn name_with_consecutive_hyphens_rejected() {
        let mut config = test_config();
        config.name = "my--app".to_string();
        let err = validate(&config).unwrap_err().to_string();
        assert!(err.contains("consecutive hyphens"));
    }

    #[test]
    fn profile_name_with_consecutive_hyphens_rejected() {
        let mut config = test_config();
        config.profiles = vec![ProfileConfig {
            name: "work--main".to_string(),
        }];
        let err = validate(&config).unwrap_err().to_string();
        assert!(err.contains("consecutive hyphens"));
    }

    #[test]
    fn url_without_scheme_rejected() {
        let mut config = test_config();
        config.url = "example.com".to_string();
        let err = validate(&config).unwrap_err().to_string();
        assert!(err.contains("http://"));
    }

    #[test]
    fn http_url_accepted() {
        let mut config = test_config();
        config.url = "http://example.com".to_string();
        assert!(validate(&config).is_ok());
    }

    #[test]
    fn zero_width_rejected() {
        let mut config = test_config();
        config.window.width = 0;
        let err = validate(&config).unwrap_err().to_string();
        assert!(err.contains("greater than zero"));
    }

    #[test]
    fn huge_dimensions_rejected() {
        let mut config = test_config();
        config.window.width = 99999;
        let err = validate(&config).unwrap_err().to_string();
        assert!(err.contains("unreasonably large"));
    }

    #[test]
    fn duplicate_profile_names_rejected() {
        let mut config = test_config();
        config.profiles = vec![
            ProfileConfig { name: "work".to_string() },
            ProfileConfig { name: "work".to_string() },
        ];
        let err = validate(&config).unwrap_err().to_string();
        assert!(err.contains("Duplicate"));
    }

    #[test]
    fn profile_name_with_special_chars_rejected() {
        let mut config = test_config();
        config.profiles = vec![ProfileConfig {
            name: "work@home".to_string(),
        }];
        let err = validate(&config).unwrap_err().to_string();
        assert!(err.contains("invalid characters"));
    }

    // -- merge tests --

    #[test]
    fn merge_scalar_override() {
        let base: serde_yaml_ng::Value = serde_yaml_ng::from_str("width: 1400").unwrap();
        let over: serde_yaml_ng::Value = serde_yaml_ng::from_str("width: 800").unwrap();
        let merged = merge_yaml(base, over);
        assert_eq!(merged["width"], serde_yaml_ng::Value::Number(800.into()));
    }

    #[test]
    fn merge_fills_gaps() {
        let base: serde_yaml_ng::Value =
            serde_yaml_ng::from_str("window:\n  width: 1400\n  height: 900").unwrap();
        let over: serde_yaml_ng::Value =
            serde_yaml_ng::from_str("name: test\nwindow:\n  height: 700").unwrap();
        let merged = merge_yaml(base, over);
        assert_eq!(merged["name"], serde_yaml_ng::Value::String("test".into()));
        assert_eq!(merged["window"]["width"], serde_yaml_ng::Value::Number(1400.into()));
        assert_eq!(merged["window"]["height"], serde_yaml_ng::Value::Number(700.into()));
    }

    #[test]
    fn merge_per_app_wins_for_scalars() {
        let base: serde_yaml_ng::Value =
            serde_yaml_ng::from_str("user_agent: global-ua").unwrap();
        let over: serde_yaml_ng::Value =
            serde_yaml_ng::from_str("name: app\nuser_agent: app-ua").unwrap();
        let merged = merge_yaml(base, over);
        assert_eq!(
            merged["user_agent"],
            serde_yaml_ng::Value::String("app-ua".into())
        );
    }

    #[test]
    fn merge_global_only_fields_preserved() {
        let base: serde_yaml_ng::Value =
            serde_yaml_ng::from_str("user_agent: global-ua\nwindow:\n  width: 1400").unwrap();
        let over: serde_yaml_ng::Value =
            serde_yaml_ng::from_str("name: app\nurl: https://example.com").unwrap();
        let merged = merge_yaml(base, over);
        assert_eq!(
            merged["user_agent"],
            serde_yaml_ng::Value::String("global-ua".into())
        );
        assert_eq!(merged["window"]["width"], serde_yaml_ng::Value::Number(1400.into()));
    }

    // -- backend tests --

    #[test]
    fn backend_defaults_to_webview() {
        let yaml = "name: test\nurl: https://example.com\n";
        let config: AppConfig = serde_yaml_ng::from_str(yaml).unwrap();
        assert_eq!(config.backend, Backend::Webview);
        assert!(!config.backend.is_browser());
    }

    #[test]
    fn backend_brave_parsed() {
        let yaml = "name: test\nurl: https://example.com\nbackend: brave\n";
        let config: AppConfig = serde_yaml_ng::from_str(yaml).unwrap();
        assert_eq!(config.backend, Backend::Brave);
        assert!(config.backend.is_browser());
    }

    #[test]
    fn backend_chrome_parsed() {
        let yaml = "name: test\nurl: https://example.com\nbackend: chrome\n";
        let config: AppConfig = serde_yaml_ng::from_str(yaml).unwrap();
        assert_eq!(config.backend, Backend::Chrome);
    }

    #[test]
    fn backend_chromium_parsed() {
        let yaml = "name: test\nurl: https://example.com\nbackend: chromium\n";
        let config: AppConfig = serde_yaml_ng::from_str(yaml).unwrap();
        assert_eq!(config.backend, Backend::Chromium);
    }

    #[test]
    fn backend_invalid_rejected() {
        let yaml = "name: test\nurl: https://example.com\nbackend: firefox\n";
        let result: Result<AppConfig, _> = serde_yaml_ng::from_str(yaml);
        assert!(result.is_err());
    }

    // -- ad-hoc tests --

    #[test]
    fn url_host_strips_scheme_and_path() {
        assert_eq!(url_host("https://web.whatsapp.com/send"), Some("web.whatsapp.com"));
        assert_eq!(url_host("http://localhost:3000"), Some("localhost:3000"));
        assert_eq!(url_host("https://example.com?q=1#x"), Some("example.com"));
        assert_eq!(url_host("ftp://nope"), None);
    }

    #[test]
    fn sanitize_name_collapses_separators() {
        assert_eq!(sanitize_name("web.whatsapp.com"), "web-whatsapp-com");
        assert_eq!(sanitize_name("localhost:3000"), "localhost-3000");
        assert_eq!(sanitize_name("a..b"), "a-b");
        assert_eq!(sanitize_name("...."), "ad-hoc");
        assert_eq!(sanitize_name("Foo.Bar"), "foo-bar");
    }

    #[test]
    fn ad_hoc_builds_valid_config() {
        let (cfg, name) = ad_hoc("https://example.com/path", Backend::Webview).unwrap();
        assert_eq!(name, "example-com");
        assert_eq!(cfg.name, "example-com");
        assert_eq!(cfg.url, "https://example.com/path");
        assert_eq!(cfg.backend, Backend::Webview);
        assert_eq!(cfg.window.title, "example.com");
    }

    #[test]
    fn ad_hoc_rejects_non_http_url() {
        assert!(ad_hoc("javascript:alert(1)", Backend::Webview).is_err());
    }

    #[test]
    fn backend_display_names() {
        assert_eq!(Backend::Webview.display_name(), "webview");
        assert_eq!(Backend::Brave.display_name(), "brave");
        assert_eq!(Backend::Chrome.display_name(), "chrome");
        assert_eq!(Backend::Chromium.display_name(), "chromium");
    }
}
