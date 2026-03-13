use crate::config::{self, AppConfig};
use anyhow::{bail, Context, Result};
use std::fs::File;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};

/// Resolve which profile to use based on the config and the user's request.
///
/// - If no profiles are defined in config, any name is allowed ("default" when unspecified).
/// - If profiles are defined and a name is requested, it must match one of them.
/// - If profiles are defined and no name is requested, the first profile is used.
pub fn resolve_name(config: &AppConfig, requested: Option<&str>) -> Result<String> {
    if config.profiles.is_empty() {
        return Ok(requested.unwrap_or(config::DEFAULT_PROFILE).to_string());
    }

    match requested {
        Some(name) => {
            if config.profiles.iter().any(|p| p.name == name) {
                Ok(name.to_string())
            } else {
                let available: Vec<_> = config.profiles.iter().map(|p| p.name.as_str()).collect();
                bail!(
                    "Profile '{}' not found. Available profiles: {}",
                    name,
                    available.join(", ")
                )
            }
        }
        None => Ok(config.profiles[0].name.clone()),
    }
}

/// Prompt the user to select a profile interactively.
/// Only call when there are multiple profiles and stdin is a terminal.
pub fn prompt_selection(config: &AppConfig) -> Result<String> {
    eprintln!("Multiple profiles available for {}:", config.name);
    for (i, p) in config.profiles.iter().enumerate() {
        eprintln!("  {}) {}", i + 1, p.name);
    }
    eprint!("Select profile [1]: ");
    std::io::stderr().flush()?;

    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    let input = input.trim();

    if input.is_empty() {
        return Ok(config.profiles[0].name.clone());
    }

    match input.parse::<usize>() {
        Ok(n) if n >= 1 && n <= config.profiles.len() => Ok(config.profiles[n - 1].name.clone()),
        _ => bail!(
            "Invalid selection '{}'. Enter a number between 1 and {}.",
            input,
            config.profiles.len()
        ),
    }
}

/// Returns true if stdin is a terminal (interactive mode).
pub fn is_interactive() -> bool {
    std::io::stdin().is_terminal()
}

pub fn data_dir(app_name: &str, profile_name: &str) -> Result<PathBuf> {
    let dirs = config::project_dirs()?;

    let profile_dir = dirs
        .data_dir()
        .join("profiles")
        .join(app_name)
        .join(profile_name);

    std::fs::create_dir_all(&profile_dir)
        .with_context(|| format!("Failed to create profile directory: {}", profile_dir.display()))?;

    Ok(profile_dir)
}

pub struct ProfileInfo {
    pub name: String,
    pub size: u64,
}

pub fn list_profiles(app_name: &str) -> Result<Vec<ProfileInfo>> {
    let dirs = config::project_dirs()?;
    let app_dir = dirs.data_dir().join("profiles").join(app_name);

    let entries = match std::fs::read_dir(&app_dir) {
        Ok(e) => e,
        Err(_) => return Ok(Vec::new()),
    };

    let mut profiles = Vec::new();
    for entry in entries {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            let name = entry.file_name().to_string_lossy().to_string();
            let size = dir_size(&entry.path()).unwrap_or(0);
            profiles.push(ProfileInfo { name, size });
        }
    }

    profiles.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(profiles)
}

pub fn clear_cache(app_name: &str, profile_name: Option<&str>) -> Result<()> {
    let dirs = config::project_dirs()?;
    let app_dir = dirs.data_dir().join("profiles").join(app_name);

    if !app_dir.exists() {
        println!("No cached data for {}.", app_name);
        return Ok(());
    }

    match profile_name {
        Some(name) => {
            let profile_dir = app_dir.join(name);
            if !profile_dir.exists() {
                println!("No cached data for {} profile '{}'.", app_name, name);
                return Ok(());
            }
            std::fs::remove_dir_all(&profile_dir).with_context(|| {
                format!("Failed to clear cache: {}", profile_dir.display())
            })?;
            println!("Cleared cache for {} profile '{}'.", app_name, name);
        }
        None => {
            std::fs::remove_dir_all(&app_dir)
                .with_context(|| format!("Failed to clear cache: {}", app_dir.display()))?;
            println!("Cleared all cached data for {}.", app_name);
        }
    }
    Ok(())
}

pub fn remove_app_data(app_name: &str) -> Result<()> {
    let dirs = config::project_dirs()?;
    let app_dir = dirs.data_dir().join("profiles").join(app_name);

    if app_dir.exists() {
        std::fs::remove_dir_all(&app_dir)
            .with_context(|| format!("Failed to remove profile data: {}", app_dir.display()))?;
        println!("Removed profile data: {}", app_dir.display());
    }
    Ok(())
}

#[derive(Debug, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct WindowState {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

pub fn load_window_state(data_dir: &Path) -> Option<WindowState> {
    let path = data_dir.join("window_state.json");
    let contents = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&contents).ok()
}

pub fn save_window_state(data_dir: &Path, state: &WindowState) {
    let path = data_dir.join("window_state.json");
    if let Ok(json) = serde_json::to_string(state) {
        let _ = std::fs::write(&path, json);
    }
}

/// Sentinel error: the lock is held by another process.
/// Callers that want to raise the existing window should match on this type.
#[derive(Debug)]
pub struct AlreadyRunning;

impl std::fmt::Display for AlreadyRunning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "another instance is already running")
    }
}

impl std::error::Error for AlreadyRunning {}

/// Acquire an exclusive lock for an app+profile combination.
/// Returns the held File (lock is released when dropped).
/// Returns `AlreadyRunning` if another instance holds the lock.
/// Returns other errors for real failures (permissions, I/O, etc.).
pub fn acquire_lock(data_dir: &Path, app_name: &str, profile_name: &str) -> Result<File> {
    use std::os::unix::io::AsRawFd;

    let lock_path = data_dir.join("lock");
    let file = File::create(&lock_path)
        .with_context(|| format!("Failed to create lock file: {}", lock_path.display()))?;

    // SAFETY: `file` is a valid, open File and `as_raw_fd()` returns a valid descriptor.
    // flock() is safe to call on any valid fd; it only manipulates advisory locks.
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if rc != 0 {
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::EWOULDBLOCK) {
            bail!(AlreadyRunning);
        }
        return Err(err).with_context(|| {
            format!(
                "Failed to lock {} (profile '{}')",
                app_name, profile_name
            )
        });
    }

    Ok(file)
}

/// Create a Unix socket listener for raising the window from a second instance.
/// Call this after acquiring the flock (so any existing socket file is stale).
pub fn create_raise_listener(data_dir: &Path) -> Result<std::os::unix::net::UnixListener> {
    use std::os::unix::net::UnixListener;
    let sock_path = data_dir.join("raise.sock");
    // Remove stale socket left by a previous crash (safe because we hold the flock)
    let _ = std::fs::remove_file(&sock_path);
    UnixListener::bind(&sock_path)
        .with_context(|| format!("Failed to create raise socket: {}", sock_path.display()))
}

/// Signal a running instance to raise its window by connecting to its socket.
pub fn signal_raise(data_dir: &Path) -> Result<()> {
    use std::os::unix::net::UnixStream;
    let sock_path = data_dir.join("raise.sock");
    let _stream = UnixStream::connect(&sock_path)
        .context("Failed to connect to running instance")?;
    Ok(())
}

fn dir_size(path: &Path) -> Result<u64> {
    let mut total = 0;
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            total += dir_size(&path)?;
        } else {
            total += entry.metadata()?.len();
        }
    }
    Ok(total)
}

pub fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProfileConfig;

    fn config_with_profiles(profiles: Vec<&str>) -> crate::config::AppConfig {
        let mut config = crate::config::test_config();
        config.profiles = profiles
            .into_iter()
            .map(|n| ProfileConfig {
                name: n.to_string(),
            })
            .collect();
        config
    }

    #[test]
    fn no_profiles_no_request_returns_default() {
        let config = config_with_profiles(vec![]);
        assert_eq!(resolve_name(&config, None).unwrap(), "default");
    }

    #[test]
    fn no_profiles_with_request_returns_requested() {
        let config = config_with_profiles(vec![]);
        assert_eq!(resolve_name(&config, Some("custom")).unwrap(), "custom");
    }

    #[test]
    fn profiles_defined_valid_request() {
        let config = config_with_profiles(vec!["personal", "work"]);
        assert_eq!(resolve_name(&config, Some("work")).unwrap(), "work");
    }

    #[test]
    fn profiles_defined_invalid_request() {
        let config = config_with_profiles(vec!["personal", "work"]);
        let err = resolve_name(&config, Some("nope")).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("nope"), "error should mention the bad name");
        assert!(msg.contains("personal"), "error should list available profiles");
        assert!(msg.contains("work"), "error should list available profiles");
    }

    #[test]
    fn profiles_defined_no_request_uses_first() {
        let config = config_with_profiles(vec!["work", "personal"]);
        assert_eq!(resolve_name(&config, None).unwrap(), "work");
    }

    #[test]
    fn save_and_load_window_state() {
        let dir = tempfile::tempdir().unwrap();
        let state = WindowState {
            x: 100,
            y: 200,
            width: 800,
            height: 600,
        };
        save_window_state(dir.path(), &state);
        let loaded = load_window_state(dir.path()).unwrap();
        assert_eq!(loaded, state);
    }

    #[test]
    fn load_window_state_returns_none_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_window_state(dir.path()).is_none());
    }

    #[test]
    fn acquire_lock_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let _lock = acquire_lock(dir.path(), "test", "default").unwrap();
        assert!(dir.path().join("lock").exists());
    }

    #[test]
    fn acquire_lock_blocks_second_instance() {
        let dir = tempfile::tempdir().unwrap();
        let _lock = acquire_lock(dir.path(), "test", "default").unwrap();
        let err = acquire_lock(dir.path(), "test", "default").unwrap_err();
        assert!(
            err.downcast_ref::<AlreadyRunning>().is_some(),
            "expected AlreadyRunning, got: {err}"
        );
    }

    #[test]
    fn acquire_lock_different_dirs_independent() {
        let dir1 = tempfile::tempdir().unwrap();
        let dir2 = tempfile::tempdir().unwrap();
        let _lock1 = acquire_lock(dir1.path(), "test", "work").unwrap();
        let _lock2 = acquire_lock(dir2.path(), "test", "personal").unwrap();
    }

    #[test]
    fn create_and_signal_raise() {
        let dir = tempfile::tempdir().unwrap();
        // Must hold the lock first (mimics real usage)
        let _lock = acquire_lock(dir.path(), "test", "default").unwrap();
        let listener = create_raise_listener(dir.path()).unwrap();
        let dir_path = dir.path().to_path_buf();
        let handle = std::thread::spawn(move || {
            signal_raise(&dir_path).unwrap();
        });
        let (_stream, _) = listener.accept().unwrap();
        handle.join().unwrap();
    }

    #[test]
    fn create_raise_listener_removes_stale_socket() {
        let dir = tempfile::tempdir().unwrap();
        let _lock = acquire_lock(dir.path(), "test", "default").unwrap();
        let listener1 = create_raise_listener(dir.path()).unwrap();
        drop(listener1);
        // Should succeed despite stale socket file from previous run
        let _listener2 = create_raise_listener(dir.path()).unwrap();
    }

    #[test]
    fn signal_raise_fails_without_listener() {
        let dir = tempfile::tempdir().unwrap();
        assert!(signal_raise(dir.path()).is_err());
    }

    #[test]
    fn format_size_bytes() {
        assert_eq!(format_size(500), "500 B");
    }

    #[test]
    fn format_size_kib() {
        assert_eq!(format_size(2048), "2.0 KiB");
    }

    #[test]
    fn format_size_mib() {
        assert_eq!(format_size(5 * 1024 * 1024), "5.0 MiB");
    }

    #[test]
    fn format_size_gib() {
        assert_eq!(format_size(2 * 1024 * 1024 * 1024), "2.0 GiB");
    }
}
