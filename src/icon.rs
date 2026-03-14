use crate::config::{self, AppConfig};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

const ICON_EXTENSIONS: &[&str] = &["png", "ico", "svg", "jpg", "jpeg", "gif", "webp"];

pub fn fetch(config: &AppConfig) -> Result<Option<PathBuf>> {
    let dirs = config::project_dirs()?;

    let icons_dir = dirs.data_dir().join("icons");
    std::fs::create_dir_all(&icons_dir)?;

    // Remove old icons (may have different extension from a previous install).
    // Ignore errors: failing to clean up stale icons is non-fatal for fetch.
    let _ = remove_from_dir(&icons_dir, &config.name);

    // Try to find the favicon URL from the page HTML, fall back to /favicon.ico
    let favicon = find_favicon_from_html(&config.url)
        .unwrap_or_else(|| fallback_favicon_url(&config.url));

    match ureq::get(&favicon).call() {
        Ok(response) => {
            let body = response
                .into_body()
                .read_to_vec()
                .context("Failed to read favicon response")?;
            let icon_path = save_as_png(&icons_dir, &config.name, &body, &favicon)?;
            println!("Fetched icon for {} from {}", config.name, favicon);
            Ok(Some(icon_path))
        }
        Err(e) => {
            eprintln!(
                "Warning: could not fetch favicon for {}: {}",
                config.name, e
            );
            Ok(None)
        }
    }
}

/// Save icon bytes as PNG for desktop environment compatibility.
/// ICO, WebP, and other formats are not reliably supported by all
/// desktop environments for .desktop file icons. If the image cannot
/// be decoded (e.g. SVG), the original bytes are saved with their
/// source extension as a best-effort fallback.
fn save_as_png(dir: &Path, name: &str, bytes: &[u8], source_url: &str) -> Result<PathBuf> {
    let icon_path = dir.join(format!("{}.png", name));

    // Try to decode and re-encode as PNG
    match image::load_from_memory(bytes) {
        Ok(img) => {
            img.save(&icon_path)
                .with_context(|| format!("Failed to save icon as PNG: {}", icon_path.display()))?;
        }
        Err(_) => {
            // Can't decode (e.g. SVG) - save with original extension
            let ext = icon_extension(source_url);
            let fallback_path = dir.join(format!("{}.{}", name, ext));
            std::fs::write(&fallback_path, bytes)?;
            return Ok(fallback_path);
        }
    }

    Ok(icon_path)
}

/// Find the cached icon path for an app, if one exists.
pub fn cached_path(app_name: &str) -> Option<PathBuf> {
    let dirs = config::project_dirs().ok()?;
    let icons_dir = dirs.data_dir().join("icons");
    for ext in ICON_EXTENSIONS {
        let path = icons_dir.join(format!("{}.{}", app_name, ext));
        if path.exists() {
            return Some(path);
        }
    }
    None
}

/// Decode an image file to raw RGBA pixel data.
/// Returns (rgba_bytes, width, height) or None on failure.
pub fn load_rgba(path: &Path) -> Option<(Vec<u8>, u32, u32)> {
    let img = image::open(path).ok()?;
    let rgba = img.into_rgba8();
    let width = rgba.width();
    let height = rgba.height();
    Some((rgba.into_raw(), width, height))
}

pub fn remove(app_name: &str) -> Result<()> {
    let dirs = config::project_dirs()?;
    let icons_dir = dirs.data_dir().join("icons");
    remove_from_dir(&icons_dir, app_name)
}

/// Remove all cached icon files for an app. Used by `uninstall` (errors
/// propagated) and `fetch` before re-fetching (errors ignored, non-fatal).
fn remove_from_dir(icons_dir: &Path, app_name: &str) -> Result<()> {
    for ext in ICON_EXTENSIONS {
        let path = icons_dir.join(format!("{}.{}", app_name, ext));
        if path.exists() {
            std::fs::remove_file(&path)
                .with_context(|| format!("Failed to remove icon: {}", path.display()))?;
        }
    }
    Ok(())
}

/// Fetch the page HTML and find the best (largest) icon.
///
/// Checks, in priority order:
/// 1. Web app manifest (`<link rel="manifest">`) icons (typically 192-512px)
/// 2. `<link rel="apple-touch-icon">` (typically 180px)
/// 3. `<link rel="icon">` with the largest `sizes` attribute
/// 4. First `<link rel="icon">` found (no sizes)
fn find_favicon_from_html(page_url: &str) -> Option<String> {
    let response = ureq::get(page_url).call().ok()?;
    let body = response.into_body().read_to_vec().ok()?;
    let html = String::from_utf8_lossy(&body);

    if let Some(url) = find_icon_from_manifest(&html, page_url) {
        return Some(url);
    }

    parse_best_icon_link(&html, page_url)
}

/// Parse the web app manifest for the largest icon.
fn find_icon_from_manifest(html: &str, page_url: &str) -> Option<String> {
    let html_lower = html.to_ascii_lowercase();
    let mut pos = 0;

    while let Some(offset) = html_lower[pos..].find("<link") {
        let start = pos + offset;
        let end = match html_lower[start..].find('>') {
            Some(e) => start + e,
            None => break,
        };

        let tag_lower = &html_lower[start..=end];
        let tag_original = &html[start..=end];

        if tag_lower.contains("rel=\"manifest\"") || tag_lower.contains("rel='manifest'") {
            if let Some(href) = extract_href(tag_original) {
                let manifest_url = resolve_url(href, page_url);
                return fetch_largest_manifest_icon(&manifest_url);
            }
        }

        pos = end + 1;
    }

    None
}

/// Fetch a web manifest JSON and return the URL of the largest icon.
fn fetch_largest_manifest_icon(manifest_url: &str) -> Option<String> {
    let response = ureq::get(manifest_url).call().ok()?;
    let body = response.into_body().read_to_vec().ok()?;
    let manifest: serde_json::Value = serde_json::from_slice(&body).ok()?;

    let icons = manifest.get("icons")?.as_array()?;
    let mut best: Option<(u32, String)> = None;

    for icon in icons {
        let Some(src) = icon.get("src").and_then(|s| s.as_str()) else {
            continue;
        };
        let size = icon
            .get("sizes")
            .and_then(|s| s.as_str())
            .and_then(parse_icon_size)
            .unwrap_or(0);

        if best.as_ref().is_none_or(|(best_size, _)| size > *best_size) {
            // Resolve against manifest_url, not page_url: manifest icon paths
            // are relative to the manifest file's location, not the page.
            best = Some((size, resolve_url(src, manifest_url)));
        }
    }

    best.map(|(_, url)| url)
}

/// Parse HTML for all icon-related `<link>` tags and return the best one.
///
/// Prefers apple-touch-icon and larger sizes over plain favicons.
fn parse_best_icon_link(html: &str, page_url: &str) -> Option<String> {
    let html_lower = html.to_ascii_lowercase();
    let mut best: Option<(u32, String)> = None;
    let mut pos = 0;

    while let Some(offset) = html_lower[pos..].find("<link") {
        let start = pos + offset;
        let end = match html_lower[start..].find('>') {
            Some(e) => start + e,
            None => break,
        };

        let tag_lower = &html_lower[start..=end];
        let tag_original = &html[start..=end];

        let is_icon = tag_lower.contains("rel=\"icon\"")
            || tag_lower.contains("rel=\"shortcut icon\"")
            || tag_lower.contains("rel='icon'")
            || tag_lower.contains("rel='shortcut icon'");

        let is_apple = tag_lower.contains("rel=\"apple-touch-icon\"")
            || tag_lower.contains("rel='apple-touch-icon'");

        if is_icon || is_apple {
            if let Some(href) = extract_href(tag_original) {
                let size = extract_attr(tag_original, "sizes")
                    .and_then(parse_icon_size)
                    // apple-touch-icon defaults to 180 when no sizes attribute
                    .unwrap_or(if is_apple { 180 } else { 0 });

                if best.as_ref().is_none_or(|(best_size, _)| size > *best_size) {
                    best = Some((size, resolve_url(href, page_url)));
                }
            }
        }

        pos = end + 1;
    }

    best.map(|(_, url)| url)
}

/// Parse a `sizes` attribute value like "192x192" into the larger dimension.
fn parse_icon_size(sizes: &str) -> Option<u32> {
    let s = sizes.split_whitespace().next()?;
    let (w, h) = s.split_once('x').or_else(|| s.split_once('X'))?;
    let w: u32 = w.parse().ok()?;
    let h: u32 = h.parse().ok()?;
    Some(w.max(h))
}

/// Extract a named attribute value from an HTML tag.
fn extract_attr<'a>(tag: &'a str, attr_name: &str) -> Option<&'a str> {
    let lower = tag.to_ascii_lowercase();
    let needle = format!("{}=", attr_name);
    let attr_pos = lower.find(&needle)?;
    let after = &tag[attr_pos + needle.len()..];

    if let Some(stripped) = after.strip_prefix('"') {
        let end = stripped.find('"')?;
        Some(&stripped[..end])
    } else if let Some(stripped) = after.strip_prefix('\'') {
        let end = stripped.find('\'')?;
        Some(&stripped[..end])
    } else {
        let end = after.find(|c: char| c.is_whitespace() || c == '>')?;
        Some(&after[..end])
    }
}

/// Extract the href attribute value from an HTML tag.
fn extract_href(tag: &str) -> Option<&str> {
    extract_attr(tag, "href")
}

/// Resolve a potentially relative URL against the page URL.
fn resolve_url(href: &str, page_url: &str) -> String {
    if href.starts_with("http://") || href.starts_with("https://") {
        return href.to_string();
    }
    if href.starts_with("//") {
        let scheme = if page_url.starts_with("https") {
            "https:"
        } else {
            "http:"
        };
        return format!("{}{}", scheme, href);
    }
    let origin = extract_origin(page_url);
    if href.starts_with('/') {
        format!("{}{}", origin, href)
    } else {
        // Find the last '/' after the scheme to use as the base directory.
        // For "https://example.com/dir/page" -> "https://example.com/dir/"
        // For "https://example.com" (no path) -> "https://example.com/"
        let after_scheme = page_url
            .find("://")
            .map(|i| i + 3)
            .unwrap_or(0);
        let base = page_url[after_scheme..]
            .rfind('/')
            .map(|i| &page_url[..after_scheme + i + 1])
            .unwrap_or_else(|| page_url);
        if base.ends_with('/') {
            format!("{}{}", base, href)
        } else {
            format!("{}/{}", base, href)
        }
    }
}

fn extract_origin(url: &str) -> &str {
    if let Some(scheme_end) = url.find("://") {
        let after_scheme = &url[scheme_end + 3..];
        if let Some(slash) = after_scheme.find('/') {
            &url[..scheme_end + 3 + slash]
        } else {
            url
        }
    } else {
        url
    }
}

fn fallback_favicon_url(url: &str) -> String {
    format!("{}/favicon.ico", extract_origin(url))
}

fn icon_extension(url: &str) -> &str {
    let path = url.split('?').next().unwrap_or(url);
    if let Some(dot) = path.rfind('.') {
        let ext = &path[dot + 1..];
        match ext {
            "png" | "ico" | "svg" | "jpg" | "jpeg" | "gif" | "webp" => ext,
            _ => "ico",
        }
    } else {
        "ico"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- parse_best_icon_link --

    #[test]
    fn parse_icon_link_double_quoted() {
        let html = r#"<html><head><link rel="icon" href="/icon.png"></head></html>"#;
        let result = parse_best_icon_link(html, "https://example.com/page");
        assert_eq!(result, Some("https://example.com/icon.png".to_string()));
    }

    #[test]
    fn parse_icon_link_single_quoted() {
        let html = "<html><head><link rel='icon' href='/icon.png'></head></html>";
        let result = parse_best_icon_link(html, "https://example.com");
        assert_eq!(result, Some("https://example.com/icon.png".to_string()));
    }

    #[test]
    fn parse_icon_link_shortcut_icon() {
        let html = r#"<link rel="shortcut icon" href="/favicon.ico">"#;
        let result = parse_best_icon_link(html, "https://example.com");
        assert_eq!(result, Some("https://example.com/favicon.ico".to_string()));
    }

    #[test]
    fn parse_icon_link_no_icon() {
        let html = r#"<link rel="stylesheet" href="/style.css">"#;
        assert_eq!(parse_best_icon_link(html, "https://example.com"), None);
    }

    #[test]
    fn parse_icon_link_absolute_href() {
        let html = r#"<link rel="icon" href="https://cdn.example.com/icon.png">"#;
        let result = parse_best_icon_link(html, "https://example.com");
        assert_eq!(result, Some("https://cdn.example.com/icon.png".to_string()));
    }

    #[test]
    fn parse_icon_link_protocol_relative() {
        let html = r#"<link rel="icon" href="//cdn.example.com/icon.png">"#;
        let result = parse_best_icon_link(html, "https://example.com");
        assert_eq!(
            result,
            Some("https://cdn.example.com/icon.png".to_string())
        );
    }

    #[test]
    fn parse_icon_link_case_insensitive() {
        let html = r#"<LINK REL="ICON" HREF="/icon.png">"#;
        let result = parse_best_icon_link(html, "https://example.com");
        assert_eq!(result, Some("https://example.com/icon.png".to_string()));
    }

    #[test]
    fn parse_icon_link_skips_non_icon_link_tags() {
        let html = r#"
            <link rel="stylesheet" href="/style.css">
            <link rel="icon" href="/real-icon.png">
        "#;
        let result = parse_best_icon_link(html, "https://example.com");
        assert_eq!(
            result,
            Some("https://example.com/real-icon.png".to_string())
        );
    }

    #[test]
    fn parse_icon_prefers_larger_size() {
        let html = r#"
            <link rel="icon" href="/small.png" sizes="16x16">
            <link rel="icon" href="/large.png" sizes="192x192">
            <link rel="icon" href="/medium.png" sizes="48x48">
        "#;
        let result = parse_best_icon_link(html, "https://example.com");
        assert_eq!(result, Some("https://example.com/large.png".to_string()));
    }

    #[test]
    fn parse_icon_prefers_apple_touch_icon() {
        let html = r#"
            <link rel="icon" href="/favicon.ico">
            <link rel="apple-touch-icon" href="/apple-icon.png">
        "#;
        let result = parse_best_icon_link(html, "https://example.com");
        assert_eq!(result, Some("https://example.com/apple-icon.png".to_string()));
    }

    #[test]
    fn parse_icon_apple_touch_with_explicit_size() {
        let html = r#"
            <link rel="apple-touch-icon" sizes="192x192" href="/large-apple.png">
            <link rel="icon" href="/favicon.ico">
        "#;
        let result = parse_best_icon_link(html, "https://example.com");
        assert_eq!(result, Some("https://example.com/large-apple.png".to_string()));
    }

    // -- parse_icon_size --

    #[test]
    fn parse_size_standard() {
        assert_eq!(parse_icon_size("192x192"), Some(192));
    }

    #[test]
    fn parse_size_rectangular() {
        assert_eq!(parse_icon_size("120x180"), Some(180));
    }

    #[test]
    fn parse_size_invalid() {
        assert_eq!(parse_icon_size("any"), None);
    }

    #[test]
    fn parse_size_multiple_spaces() {
        assert_eq!(parse_icon_size("192x192 512x512"), Some(192));
    }

    // -- extract_href --

    #[test]
    fn extract_href_double_quotes() {
        assert_eq!(
            extract_href(r#"<link rel="icon" href="/icon.png">"#),
            Some("/icon.png")
        );
    }

    #[test]
    fn extract_href_single_quotes() {
        assert_eq!(
            extract_href("<link rel='icon' href='/icon.png'>"),
            Some("/icon.png")
        );
    }

    #[test]
    fn extract_href_missing() {
        assert_eq!(extract_href(r#"<link rel="icon">"#), None);
    }

    // -- resolve_url --

    #[test]
    fn resolve_url_absolute() {
        assert_eq!(
            resolve_url("https://other.com/icon.png", "https://example.com"),
            "https://other.com/icon.png"
        );
    }

    #[test]
    fn resolve_url_protocol_relative_https() {
        assert_eq!(
            resolve_url("//cdn.example.com/icon.png", "https://example.com"),
            "https://cdn.example.com/icon.png"
        );
    }

    #[test]
    fn resolve_url_protocol_relative_http() {
        assert_eq!(
            resolve_url("//cdn.example.com/icon.png", "http://example.com"),
            "http://cdn.example.com/icon.png"
        );
    }

    #[test]
    fn resolve_url_absolute_path() {
        assert_eq!(
            resolve_url("/assets/icon.png", "https://example.com/page/here"),
            "https://example.com/assets/icon.png"
        );
    }

    #[test]
    fn resolve_url_relative_path() {
        assert_eq!(
            resolve_url("icon.png", "https://example.com/page/"),
            "https://example.com/page/icon.png"
        );
    }

    #[test]
    fn resolve_url_relative_no_path_in_base() {
        // "https://example.com" + "manifest.json" should not land in the scheme
        assert_eq!(
            resolve_url("manifest.json", "https://example.com"),
            "https://example.com/manifest.json"
        );
    }

    // -- icon_extension --

    #[test]
    fn icon_extension_png() {
        assert_eq!(icon_extension("https://example.com/icon.png"), "png");
    }

    #[test]
    fn icon_extension_ico() {
        assert_eq!(icon_extension("https://example.com/favicon.ico"), "ico");
    }

    #[test]
    fn icon_extension_svg() {
        assert_eq!(icon_extension("https://example.com/icon.svg"), "svg");
    }

    #[test]
    fn icon_extension_strips_query_string() {
        assert_eq!(
            icon_extension("https://example.com/icon.png?v=2"),
            "png"
        );
    }

    #[test]
    fn icon_extension_unknown_defaults_to_ico() {
        assert_eq!(icon_extension("https://example.com/icon.bmp"), "ico");
    }

    #[test]
    fn icon_extension_no_extension_defaults_to_ico() {
        assert_eq!(icon_extension("https://example.com/favicon"), "ico");
    }

    // -- fallback_favicon_url --

    #[test]
    fn fallback_url_with_path() {
        assert_eq!(
            fallback_favicon_url("https://example.com/some/page"),
            "https://example.com/favicon.ico"
        );
    }

    #[test]
    fn fallback_url_no_path() {
        assert_eq!(
            fallback_favicon_url("https://example.com"),
            "https://example.com/favicon.ico"
        );
    }
}
