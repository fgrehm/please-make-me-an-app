mod adblock;
mod app;
mod browser;
mod config;
mod desktop;
mod icon;
mod inject;
mod notification;
mod profile;
mod tray;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

fn long_version() -> &'static str {
    const VERSION: &str = concat!(
        env!("PMMA_VERSION"),
        "\ncommit: ",
        env!("PMMA_GIT_HASH"),
        "\nbuilt:  ",
        env!("PMMA_BUILD_DATE"),
    );
    VERSION
}

#[derive(Parser)]
#[command(
    name = "please-make-me-an-app",
    about = "Turn any website into a desktop app",
    long_about = "Turn any website into a standalone desktop app with its own window, icon, \
                  and launcher entry. Write a YAML config, run a command, done.\n\n\
                  Each app gets profile-isolated storage (cookies, localStorage, cache), \
                  optional CSS/JS injection, ad blocking, system notifications, and a \
                  system tray icon.",
    version = env!("PMMA_VERSION"),
    long_version = long_version(),
    after_help = "Examples:\n  \
                  please-make-me-an-app open whatsapp.yaml\n  \
                  please-make-me-an-app open gmail.yaml --profile work\n  \
                  please-make-me-an-app install whatsapp.yaml\n  \
                  please-make-me-an-app list\n  \
                  please-make-me-an-app clear-cache gmail --profile work\n  \
                  please-make-me-an-app uninstall gmail --purge\n  \
                  please-make-me-an-app uninstall --all",
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Open a web app from a config file
    #[command(
        long_about = "Open a web app in a standalone native window.\n\n\
                      If the config defines multiple profiles and none is specified, \
                      an interactive picker is shown. Only one instance per app+profile \
                      can run at a time.",
        after_help = "Examples:\n  \
                      please-make-me-an-app open whatsapp.yaml\n  \
                      please-make-me-an-app open gmail.yaml -p work\n  \
                      please-make-me-an-app open slack.yaml --debug",
    )]
    Open {
        /// Path to the app config YAML file
        config: PathBuf,

        /// Profile name to use
        ///
        /// Selects a named profile for session isolation. If the config defines
        /// profiles and this is omitted, an interactive picker is shown (or the
        /// first profile is used in non-interactive mode).
        #[arg(short, long)]
        profile: Option<String>,

        /// Navigate to this URL on launch instead of the configured URL
        ///
        /// Used by URL scheme handlers (e.g. whatsapp://) to deep-link into the
        /// app. Custom schemes are rewritten to the configured base URL:
        /// whatsapp://send/?phone=1234 + https://web.whatsapp.com
        ///   -> https://web.whatsapp.com/send/?phone=1234
        #[arg(long)]
        url: Option<String>,

        /// Print debug info to stderr
        ///
        /// Logs config values, user agent, data directory, and injects a script
        /// that reports navigator properties via IPC.
        #[arg(long)]
        debug: bool,
    },

    /// Install a web app to the desktop launcher
    ///
    /// Copies the config to the XDG config directory, fetches the site's
    /// favicon, and generates a .desktop file so the app appears in your
    /// application menu. Pass a config file path for first install, or
    /// just the app name to reinstall (re-fetch icon, regenerate .desktop).
    #[command(
        after_help = "Examples:\n  \
                      please-make-me-an-app install whatsapp.yaml\n  \
                      please-make-me-an-app install examples/gmail.yaml\n  \
                      please-make-me-an-app install gmail  # reinstall",
    )]
    Install {
        /// Path to config YAML file, or app name to reinstall
        config: String,
    },

    /// List all installed web apps
    ///
    /// Shows each installed app with its config path, profiles, and storage
    /// size.
    List,

    /// Uninstall a web app
    ///
    /// Removes the .desktop launcher entry. Use --purge to also delete all
    /// profile data (cookies, cache, localStorage) and the cached icon.
    /// Use --all to uninstall every installed app and remove all data
    /// (prompts for confirmation).
    #[command(
        after_help = "Examples:\n  \
                      please-make-me-an-app uninstall gmail\n  \
                      please-make-me-an-app uninstall gmail --purge\n  \
                      please-make-me-an-app uninstall --all",
    )]
    Uninstall {
        /// Name of the app to uninstall (as defined in the YAML config)
        #[arg(required_unless_present = "all", conflicts_with = "all")]
        name: Option<String>,

        /// Also remove all profile data and cached icon
        #[arg(long, conflicts_with = "all")]
        purge: bool,

        /// Uninstall all installed apps and remove all data (prompts for confirmation)
        #[arg(long)]
        all: bool,
    },

    /// Clear cached web data (cookies, localStorage, cache) for an app
    ///
    /// Without --profile, clears data for all profiles. With --profile, only
    /// the specified profile is cleared.
    #[command(
        after_help = "Examples:\n  \
                      please-make-me-an-app clear-cache gmail\n  \
                      please-make-me-an-app clear-cache gmail -p work",
    )]
    ClearCache {
        /// Name of the app to clear (as defined in the YAML config)
        name: String,

        /// Only clear a specific profile (clears all profiles if omitted)
        #[arg(short, long)]
        profile: Option<String>,
    },
}

/// Resolve the install config argument to a file path.
///
/// If the argument looks like a file path (ends in .yaml/.yml or contains a separator),
/// use it directly. Otherwise treat it as an app name and look up the config path
/// from the installed .desktop file, allowing `install gmail` to reinstall.
fn resolve_install_config(config: &str) -> Result<PathBuf> {
    let path = Path::new(config);
    if config.ends_with(".yaml")
        || config.ends_with(".yml")
        || config.contains(std::path::MAIN_SEPARATOR)
    {
        return Ok(path.to_path_buf());
    }

    // Treat as app name: check XDG config dir first, then installed .desktop files
    let dirs = config::project_dirs()?;
    let xdg_config = dirs.config_dir().join("apps").join(format!("{}.yaml", config));
    if xdg_config.exists() {
        return Ok(xdg_config);
    }

    // Fall back to parsing the Exec line from the .desktop file
    let apps = desktop::list_installed()?;
    for app in &apps {
        if app.name == config {
            if let Some(ref config_path) = app.config_path {
                return Ok(PathBuf::from(config_path));
            }
        }
    }

    anyhow::bail!(
        "No installed app named '{}'. Pass a config file path for first install.",
        config
    );
}

/// Copy the config file to the XDG config directory and return the destination path.
fn install_config(app_name: &str, config_path: &Path) -> Result<PathBuf> {
    let dirs = config::project_dirs()?;
    let config_dir = dirs.config_dir().join("apps");
    std::fs::create_dir_all(&config_dir)
        .with_context(|| format!("Failed to create config directory: {}", config_dir.display()))?;
    let dest = config_dir.join(format!("{}.yaml", app_name));
    let src = std::fs::canonicalize(config_path)
        .with_context(|| format!("Failed to resolve config path: {}", config_path.display()))?;
    if src != dest {
        std::fs::copy(&src, &dest)
            .with_context(|| format!("Failed to copy config to {}", dest.display()))?;
        println!("Config saved to {}", dest.display());
    }
    Ok(dest)
}

/// Resolve the effective URL for app launch.
///
/// When `url_override` is a custom scheme (e.g. `whatsapp://send/?phone=123`),
/// rewrite it to an https URL under `base_url` by stripping the scheme and
/// treating the rest as a path:
///   whatsapp://send/?phone=123 + https://web.whatsapp.com
///     -> https://web.whatsapp.com/send/?phone=123
///
/// http/https overrides are used as-is. Empty overrides fall back to `base_url`.
fn resolve_url<'a>(base_url: &'a str, url_override: Option<&str>) -> std::borrow::Cow<'a, str> {
    let Some(override_url) = url_override.filter(|u| !u.is_empty()) else {
        return std::borrow::Cow::Borrowed(base_url);
    };
    if override_url.starts_with("http://") || override_url.starts_with("https://") {
        return std::borrow::Cow::Owned(override_url.to_string());
    }
    // Custom scheme: strip "scheme://" and append to base URL.
    if let Some(after_scheme) = override_url.find("://").map(|i| &override_url[i + 3..]) {
        let base = base_url.trim_end_matches('/');
        return std::borrow::Cow::Owned(format!("{}/{}", base, after_scheme));
    }
    std::borrow::Cow::Borrowed(base_url)
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Open {
            config,
            profile,
            url,
            debug,
        } => {
            let app_config = config::load(&config)?;
            let profile_name = if profile.is_none()
                && app_config.profiles.len() > 1
                && profile::is_interactive()
            {
                profile::prompt_selection(&app_config)?
            } else {
                profile::resolve_name(&app_config, profile.as_deref())?
            };
            let data_dir = profile::data_dir(&app_config.name, &profile_name)?;
            let effective_url = resolve_url(&app_config.url, url.as_deref());

            if debug {
                eprintln!("[debug] app: {}", app_config.name);
                eprintln!("[debug] url: {}", effective_url);
                eprintln!("[debug] backend: {}", app_config.backend.display_name());
                eprintln!("[debug] profile: {}", profile_name);
                eprintln!("[debug] data_dir: {}", data_dir.display());
            }

            if app_config.backend.is_browser() {
                browser::warn_ignored_options(&app_config);
                if debug {
                    match browser::find_binary(&app_config.backend) {
                        Ok(path) => eprintln!("[debug] browser binary: {}", path.display()),
                        Err(e) => eprintln!("[debug] browser binary: {}", e),
                    }
                }
                browser::run(&app_config, &profile_name, &data_dir, &effective_url)?;
            } else {
                let config_dir = config.parent().unwrap_or_else(|| Path::new("."));
                if debug {
                    eprintln!(
                        "[debug] user_agent: {}",
                        app_config
                            .user_agent
                            .as_deref()
                            .unwrap_or("(system default)")
                    );
                    eprintln!("[debug] clipboard: {}", app_config.clipboard);
                    eprintln!("[debug] adblock: {}", app_config.adblock);
                    eprintln!("[debug] notifications: {}", app_config.notifications);
                    eprintln!(
                        "[debug] tray: enabled={} minimize_to_tray={}",
                        app_config.tray.enabled, app_config.tray.minimize_to_tray
                    );
                    eprintln!(
                        "[debug] allowed_domains: {:?}",
                        app_config.allowed_domains
                    );
                    if app_config.inject.css.is_some() || app_config.inject.css_file.is_some() {
                        eprintln!("[debug] inject: CSS active");
                    }
                    if app_config.inject.js.is_some() || app_config.inject.js_file.is_some() {
                        eprintln!("[debug] inject: JS active");
                    }
                }
                let _lock = match profile::acquire_lock(
                    &data_dir,
                    &app_config.name,
                    &profile_name,
                ) {
                    Ok(lock) => lock,
                    Err(_) => {
                        if profile::signal_raise(&data_dir).is_ok() {
                            println!(
                                "{} (profile '{}') is already running. Raised existing window.",
                                app_config.name, profile_name
                            );
                        } else {
                            println!(
                                "{} (profile '{}') is already running.",
                                app_config.name, profile_name
                            );
                        }
                        return Ok(());
                    }
                };
                app::run(&app_config, &profile_name, &data_dir, config_dir, debug, &effective_url)?;
            }
        }
        Commands::Install { config } => {
            let config_path = resolve_install_config(&config)?;
            let app_config = config::load(&config_path)?;
            if app_config.backend.is_browser() {
                browser::warn_ignored_options(&app_config);
            }
            let installed_config = install_config(&app_config.name, &config_path)?;
            let icon_path = icon::fetch(&app_config)?;
            desktop::generate(&app_config, &installed_config, icon_path.as_deref())?;
            if app_config.profiles.is_empty() {
                println!("Installed {} to your application launcher.", app_config.name);
            } else {
                let names: Vec<_> =
                    app_config.profiles.iter().map(|p| p.name.as_str()).collect();
                println!(
                    "Installed {} to your application launcher (profiles: {}).",
                    app_config.name,
                    names.join(", ")
                );
            }
        }
        Commands::List => {
            let apps = desktop::list_installed()?;
            if apps.is_empty() {
                println!("No installed apps. Use 'install' to add one.");
                return Ok(());
            }
            for app in &apps {
                println!("{}", app.name);
                if let Some(config_path) = &app.config_path {
                    println!("  Config: {}", config_path);
                }
                let profiles = profile::list_profiles(&app.name)?;
                if !profiles.is_empty() {
                    println!("  Profiles:");
                    for p in &profiles {
                        println!("    {:<16} {}", p.name, profile::format_size(p.size));
                    }
                }
            }
        }
        Commands::Uninstall { name, purge, all } => {
            if all {
                let apps = desktop::list_installed()?;
                if apps.is_empty() {
                    println!("No installed apps.");
                    return Ok(());
                }
                println!(
                    "The following apps will be uninstalled and all data removed:"
                );
                for app in &apps {
                    println!("  {}", app.name);
                }
                if !profile::is_interactive() {
                    anyhow::bail!(
                        "--all requires an interactive terminal; uninstall apps individually instead"
                    );
                }
                print!("Continue? [y/N] ");
                std::io::stdout().flush()?;
                let mut input = String::new();
                std::io::stdin().lock().read_line(&mut input)?;
                if input.trim().to_lowercase() != "y" {
                    println!("Aborted.");
                    return Ok(());
                }
                for app in &apps {
                    desktop::uninstall(&app.name)?;
                    profile::remove_app_data(&app.name)?;
                    icon::remove(&app.name)?;
                    println!("Uninstalled {}.", app.name);
                }
            } else {
                let name = name.expect("name is required when --all is not set");
                desktop::uninstall(&name)?;
                if purge {
                    profile::remove_app_data(&name)?;
                    icon::remove(&name)?;
                }
                println!("Uninstalled {}.", name);
            }
        }
        Commands::ClearCache { name, profile } => {
            profile::clear_cache(&name, profile.as_deref())?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn resolve_url_no_override_returns_base() {
        assert_eq!(resolve_url("https://web.whatsapp.com", None), "https://web.whatsapp.com");
    }

    #[test]
    fn resolve_url_empty_override_returns_base() {
        assert_eq!(resolve_url("https://web.whatsapp.com", Some("")), "https://web.whatsapp.com");
    }

    #[test]
    fn resolve_url_https_override_used_as_is() {
        assert_eq!(
            resolve_url("https://web.whatsapp.com", Some("https://other.com/page")),
            "https://other.com/page"
        );
    }

    #[test]
    fn resolve_url_custom_scheme_rewritten_to_base() {
        assert_eq!(
            resolve_url("https://web.whatsapp.com", Some("whatsapp://send/?phone=123&text=hi")),
            "https://web.whatsapp.com/send/?phone=123&text=hi"
        );
    }

    #[test]
    fn resolve_url_base_trailing_slash_stripped() {
        assert_eq!(
            resolve_url("https://web.whatsapp.com/", Some("whatsapp://send/?phone=123")),
            "https://web.whatsapp.com/send/?phone=123"
        );
    }

    #[test]
    fn install_config_copies_file_to_xdg_dir() {
        let src_dir = tempfile::tempdir().unwrap();
        let src = src_dir.path().join("myapp.yaml");
        fs::write(&src, "name: myapp\nurl: https://example.com\n").unwrap();

        let dest = install_config("myapp", &src).unwrap();

        assert!(dest.exists());
        assert_eq!(
            fs::read_to_string(&dest).unwrap(),
            "name: myapp\nurl: https://example.com\n"
        );
        assert!(dest.ends_with("apps/myapp.yaml"));

        // Cleanup
        fs::remove_file(&dest).ok();
    }

    #[test]
    fn install_config_skips_copy_when_src_equals_dest() {
        // Simulate the case where the config is already in the XDG config dir.
        // We can't easily reproduce the exact XDG path in a unit test, but we
        // can verify install_config returns Ok with the destination path.
        let src_dir = tempfile::tempdir().unwrap();
        let src = src_dir.path().join("idempotent.yaml");
        fs::write(&src, "name: idempotent\nurl: https://example.com\n").unwrap();

        let dest1 = install_config("idempotent", &src).unwrap();
        // Calling again should not error (idempotent).
        let dest2 = install_config("idempotent", &src).unwrap();
        assert_eq!(dest1, dest2);

        // Cleanup
        fs::remove_file(&dest1).ok();
    }

    #[test]
    fn install_config_returns_error_for_missing_source() {
        let result = install_config("missing", Path::new("/nonexistent/missing.yaml"));
        assert!(result.is_err());
    }
}
