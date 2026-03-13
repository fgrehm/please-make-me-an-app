use std::process::Command;

fn main() {
    // Git commit hash (short)
    let git_hash = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    // Git dirty flag
    let git_dirty = Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false);

    let git_info = if git_dirty {
        format!("{}-dirty", git_hash)
    } else {
        git_hash
    };

    // Build timestamp (UTC, YYYY-MM-DD HH:MM:SS)
    let build_date = Command::new("date")
        .args(["-u", "+%Y-%m-%d %H:%M:%S UTC"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    // Version from git tag (e.g., "v0.1.2" -> "0.1.2"), falls back to Cargo.toml version.
    // CI builds from a tag get the tag version; local builds get Cargo.toml version.
    let version = Command::new("git")
        .args(["describe", "--tags", "--exact-match"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            let tag = String::from_utf8_lossy(&o.stdout).trim().to_string();
            tag.strip_prefix('v').unwrap_or(&tag).to_string()
        })
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());

    println!("cargo:rustc-env=PMMA_VERSION={}", version);
    println!("cargo:rustc-env=PMMA_GIT_HASH={}", git_info);
    println!("cargo:rustc-env=PMMA_BUILD_DATE={}", build_date);
}
