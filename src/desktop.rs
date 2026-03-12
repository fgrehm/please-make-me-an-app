use crate::config::AppConfig;
use anyhow::{bail, Context, Result};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub struct InstalledApp {
    pub name: String,
    pub config_path: Option<String>,
}

pub fn generate(
    config: &AppConfig,
    config_path: &Path,
    icon_path: Option<&Path>,
) -> Result<()> {
    let applications_dir = desktop_entry_dir()?;
    std::fs::create_dir_all(&applications_dir)?;

    let config_path_abs = std::fs::canonicalize(config_path)
        .with_context(|| format!("Failed to resolve config path: {}", config_path.display()))?;

    let binary = std::env::current_exe().context("Failed to determine binary path")?;

    // Remove any existing desktop files for this app before writing new ones.
    // Handles profile additions/removals between installs.
    remove_desktop_files(&applications_dir, &config.name);

    if config.profiles.is_empty() {
        write_desktop_entry(&applications_dir, config, &binary, &config_path_abs, icon_path, None)?;
    } else {
        for profile in &config.profiles {
            write_desktop_entry(
                &applications_dir,
                config,
                &binary,
                &config_path_abs,
                icon_path,
                Some(&profile.name),
            )?;
        }
    }

    update_mime_database(&applications_dir);

    Ok(())
}

fn write_desktop_entry(
    dir: &Path,
    config: &AppConfig,
    binary: &Path,
    config_path: &Path,
    icon_path: Option<&Path>,
    profile: Option<&str>,
) -> Result<()> {
    let app_name = &config.name;
    let title = &config.window.title;

    let (filename, display_name, profile_arg, log_suffix) = match profile {
        Some(p) => (
            format!("pmma-{}--{}.desktop", app_name, p),
            format!("{} ({})", title, p),
            format!(" --profile {}", p),
            format!("{}-{}", app_name, p),
        ),
        None => (
            format!("pmma-{}.desktop", app_name),
            title.to_string(),
            String::new(),
            app_name.to_string(),
        ),
    };

    let desktop_file = dir.join(&filename);
    let icon_line = match icon_path {
        Some(p) => format!("Icon={}", p.display()),
        None => String::new(),
    };
    let log_file = format!("/tmp/pmma-{}.log", log_suffix);
    let wm_class_line = if config.backend.is_browser() {
        let class = match profile {
            Some(p) => format!("pmma-{}--{}", app_name, p),
            None => format!("pmma-{}", app_name),
        };
        format!("StartupWMClass={}\n", class)
    } else {
        String::new()
    };

    // When url_schemes are registered, pass the activated URL as $1 via %u.
    // sh -c 'script' sh %u  =>  $0=sh, $1=<url from %u>
    // When launched from the app menu (no URL), %u is omitted and $1 is empty.
    let exec_line = if config.url_schemes.is_empty() {
        format!(
            "Exec=sh -c '{binary} open {config}{profile_arg} >>{log} 2>&1'",
            binary = binary.display(),
            config = config_path.display(),
            profile_arg = profile_arg,
            log = log_file,
        )
    } else {
        format!(
            "Exec=sh -c '{binary} open {config}{profile_arg} --url \"$1\" >>{log} 2>&1' sh %u",
            binary = binary.display(),
            config = config_path.display(),
            profile_arg = profile_arg,
            log = log_file,
        )
    };

    let mime_line = if config.url_schemes.is_empty() {
        String::new()
    } else {
        let mime_types: String = config
            .url_schemes
            .iter()
            .map(|s| format!("x-scheme-handler/{};", s))
            .collect();
        format!("MimeType={}\n", mime_types)
    };

    let contents = format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name={name}\n\
         {exec}\n\
         {icon}\n\
         {mime}\
         {wm_class}\
         Terminal=false\n\
         Categories=Network;\n",
        name = display_name,
        exec = exec_line,
        icon = icon_line,
        mime = mime_line,
        wm_class = wm_class_line,
    );

    std::fs::write(&desktop_file, contents)
        .with_context(|| format!("Failed to write desktop file: {}", desktop_file.display()))?;

    println!("Created {}", desktop_file.display());
    Ok(())
}

/// Remove all desktop files for an app (base file and per-profile files).
fn remove_desktop_files(dir: &Path, app_name: &str) {
    let base = dir.join(format!("pmma-{}.desktop", app_name));
    if base.exists() {
        let _ = std::fs::remove_file(&base);
    }

    let prefix = format!("pmma-{}--", app_name);
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let filename = entry.file_name().to_string_lossy().to_string();
            if filename.starts_with(&prefix) && filename.ends_with(".desktop") {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

pub fn list_installed() -> Result<Vec<InstalledApp>> {
    let dir = desktop_entry_dir()?;

    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return Ok(Vec::new()),
    };

    let mut seen = HashSet::new();
    let mut apps = Vec::new();
    for entry in entries {
        let entry = entry?;
        let filename = entry.file_name().to_string_lossy().to_string();

        if let Some(rest) = filename
            .strip_prefix("pmma-")
            .and_then(|s| s.strip_suffix(".desktop"))
        {
            // Extract app name, stripping profile suffix if present
            let app_name = match rest.split_once("--") {
                Some((name, _)) => name,
                None => rest,
            };
            if seen.insert(app_name.to_string()) {
                let contents = std::fs::read_to_string(entry.path()).unwrap_or_default();
                let config_path = parse_exec_config(&contents);
                apps.push(InstalledApp {
                    name: app_name.to_string(),
                    config_path,
                });
            }
        }
    }

    apps.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(apps)
}

pub fn uninstall(name: &str) -> Result<()> {
    let dir = desktop_entry_dir()?;
    let mut removed = false;

    let base_file = dir.join(format!("pmma-{}.desktop", name));
    if base_file.exists() {
        std::fs::remove_file(&base_file)
            .with_context(|| format!("Failed to remove {}", base_file.display()))?;
        println!("Removed {}", base_file.display());
        removed = true;
    }

    let prefix = format!("pmma-{}--", name);
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let filename = entry.file_name().to_string_lossy().to_string();
            if filename.starts_with(&prefix) && filename.ends_with(".desktop") {
                std::fs::remove_file(entry.path())
                    .with_context(|| format!("Failed to remove {}", entry.path().display()))?;
                println!("Removed {}", entry.path().display());
                removed = true;
            }
        }
    }

    if !removed {
        bail!(
            "No installed app named '{}'. Run 'list' to see installed apps.",
            name
        );
    }

    Ok(())
}

/// Extract the config file path from an Exec= line like:
///   Exec=sh -c '/path/to/binary open /path/to/config.yaml >>/tmp/pmma-app.log 2>&1'
///   Exec=sh -c '/path/to/binary open /path/to/config.yaml --profile work >>/tmp/pmma-app-work.log 2>&1'
///   Exec=/path/to/binary open /path/to/config.yaml
fn parse_exec_config(desktop_contents: &str) -> Option<String> {
    for line in desktop_contents.lines() {
        if let Some(exec) = line.strip_prefix("Exec=") {
            if let Some(after_open) = exec.split_once(" open ") {
                let rest = after_open.1;
                let rest = rest.split(" >>").next().unwrap_or(rest);
                let rest = rest.split(" --profile").next().unwrap_or(rest);
                return Some(rest.to_string());
            }
        }
    }
    None
}

/// Run update-desktop-database so URL scheme handlers take effect immediately.
/// This is a best-effort call; failure is non-fatal.
fn update_mime_database(dir: &Path) {
    let status = std::process::Command::new("update-desktop-database")
        .arg(dir)
        .status();
    match status {
        Ok(s) if s.success() => {}
        Ok(s) => eprintln!(
            "warning: update-desktop-database exited with status {}",
            s
        ),
        Err(e) => eprintln!("warning: update-desktop-database not found: {}", e),
    }
}

fn desktop_entry_dir() -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME environment variable not set")?;
    Ok(PathBuf::from(home).join(".local/share/applications"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_exec_config_extracts_path() {
        let contents = "[Desktop Entry]\n\
                         Type=Application\n\
                         Name=gmail\n\
                         Exec=sh -c '/usr/local/bin/please-make-me-an-app open /home/user/.config/please-make-me-an-app/apps/gmail.yaml >>/tmp/pmma-gmail.log 2>&1'\n\
                         Terminal=false\n";
        assert_eq!(
            parse_exec_config(contents),
            Some("/home/user/.config/please-make-me-an-app/apps/gmail.yaml".to_string())
        );
    }

    #[test]
    fn parse_exec_config_strips_profile_flag() {
        let contents = "[Desktop Entry]\n\
                         Type=Application\n\
                         Name=Gmail (work)\n\
                         Exec=sh -c '/usr/local/bin/please-make-me-an-app open /home/user/.config/please-make-me-an-app/apps/gmail.yaml --profile work >>/tmp/pmma-gmail-work.log 2>&1'\n\
                         Terminal=false\n";
        assert_eq!(
            parse_exec_config(contents),
            Some("/home/user/.config/please-make-me-an-app/apps/gmail.yaml".to_string())
        );
    }

    #[test]
    fn parse_exec_config_legacy_format() {
        let contents = "[Desktop Entry]\n\
                         Type=Application\n\
                         Name=gmail\n\
                         Exec=/usr/local/bin/please-make-me-an-app open /home/user/.config/please-make-me-an-app/apps/gmail.yaml\n\
                         Terminal=false\n";
        assert_eq!(
            parse_exec_config(contents),
            Some("/home/user/.config/please-make-me-an-app/apps/gmail.yaml".to_string())
        );
    }

    #[test]
    fn parse_exec_config_no_exec_line() {
        let contents = "[Desktop Entry]\nType=Application\n";
        assert_eq!(parse_exec_config(contents), None);
    }
}
