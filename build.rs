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

    // Version resolution, in priority order:
    // 1. Exact tag on HEAD (CI release builds) -> "0.3.0"
    // 2. git describe from nearest tag (local/dev builds) -> "0.3.0-5-gabcdef"
    // 3. No tags at all -> "dev"  (Cargo.toml version is not used)
    let version = Command::new("git")
        .args(["describe", "--tags", "--exact-match"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            let tag = String::from_utf8_lossy(&o.stdout).trim().to_string();
            tag.strip_prefix('v').unwrap_or(&tag).to_string()
        })
        .or_else(|| {
            Command::new("git")
                .args(["describe", "--tags", "--long"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| {
                    // "v0.1.3-5-gabcdef" -> "0.1.3-5-gabcdef"
                    let desc = String::from_utf8_lossy(&o.stdout).trim().to_string();
                    desc.strip_prefix('v').unwrap_or(&desc).to_string()
                })
        })
        .unwrap_or_else(|| "dev".to_string());

    println!("cargo:rustc-env=PMMA_VERSION={}", version);
    println!("cargo:rustc-env=PMMA_GIT_HASH={}", git_info);
    println!("cargo:rustc-env=PMMA_BUILD_DATE={}", build_date);

    // Re-run when git state changes (new commits, tags, branch switches)
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/tags");
    println!("cargo:rerun-if-changed=.git/refs/heads");
}
