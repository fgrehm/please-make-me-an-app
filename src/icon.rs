use crate::config::{self, AppConfig};
use anyhow::{Context, Result};
use std::path::PathBuf;

const ICON_EXTENSIONS: &[&str] = &["png", "ico", "svg", "jpg", "jpeg", "gif", "webp"];

pub fn fetch(config: &AppConfig) -> Result<Option<PathBuf>> {
    let dirs = config::project_dirs()?;

    let icons_dir = dirs.data_dir().join("icons");
    std::fs::create_dir_all(&icons_dir)?;

    // Try to find the favicon URL from the page HTML, fall back to /favicon.ico
    let favicon = find_favicon_from_html(&config.url)
        .unwrap_or_else(|| fallback_favicon_url(&config.url));

    let ext = icon_extension(&favicon);
    let icon_path = icons_dir.join(format!("{}.{}", config.name, ext));

    match ureq::get(&favicon).call() {
        Ok(response) => {
            let body = response
                .into_body()
                .read_to_vec()
                .context("Failed to read favicon response")?;
            std::fs::write(&icon_path, &body)?;
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

pub fn remove(app_name: &str) -> Result<()> {
    let dirs = config::project_dirs()?;

    let icons_dir = dirs.data_dir().join("icons");
    for ext in ICON_EXTENSIONS {
        let path = icons_dir.join(format!("{}.{}", app_name, ext));
        if path.exists() {
            std::fs::remove_file(&path)?;
            println!("Removed icon: {}", path.display());
        }
    }
    Ok(())
}

/// Fetch the page HTML and look for a <link rel="icon"> tag.
fn find_favicon_from_html(page_url: &str) -> Option<String> {
    let response = ureq::get(page_url).call().ok()?;
    let body = response.into_body().read_to_vec().ok()?;
    let html = String::from_utf8_lossy(&body);
    parse_icon_link(&html, page_url)
}

/// Parse HTML for <link> tags with rel="icon" or rel="shortcut icon" and extract the href.
fn parse_icon_link(html: &str, page_url: &str) -> Option<String> {
    // html_lower and html are indexed with the same byte offsets. This is safe
    // because HTML tag names and attribute names are ASCII: to_lowercase() is
    // byte-for-byte on ASCII, so offsets in html_lower map directly to html.
    let html_lower = html.to_lowercase();
    let mut pos = 0;

    while let Some(offset) = html_lower[pos..].find("<link") {
        let start = pos + offset;
        let end = match html_lower[start..].find('>') {
            Some(e) => start + e,
            None => break,
        };

        let tag_lower = &html_lower[start..=end];
        let tag_original = &html[start..=end];

        if tag_lower.contains("rel=\"icon\"")
            || tag_lower.contains("rel=\"shortcut icon\"")
            || tag_lower.contains("rel='icon'")
            || tag_lower.contains("rel='shortcut icon'")
        {
            if let Some(href) = extract_href(tag_original) {
                return Some(resolve_url(href, page_url));
            }
        }

        pos = end + 1;
    }

    None
}

/// Extract the href attribute value from an HTML tag.
fn extract_href(tag: &str) -> Option<&str> {
    let lower = tag.to_lowercase();
    let href_pos = lower.find("href=")?;
    let after = &tag[href_pos + 5..];

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
        let base = page_url
            .rfind('/')
            .map(|i| &page_url[..=i])
            .unwrap_or(page_url);
        format!("{}{}", base, href)
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

    // -- parse_icon_link --

    #[test]
    fn parse_icon_link_double_quoted() {
        let html = r#"<html><head><link rel="icon" href="/icon.png"></head></html>"#;
        let result = parse_icon_link(html, "https://example.com/page");
        assert_eq!(result, Some("https://example.com/icon.png".to_string()));
    }

    #[test]
    fn parse_icon_link_single_quoted() {
        let html = "<html><head><link rel='icon' href='/icon.png'></head></html>";
        let result = parse_icon_link(html, "https://example.com");
        assert_eq!(result, Some("https://example.com/icon.png".to_string()));
    }

    #[test]
    fn parse_icon_link_shortcut_icon() {
        let html = r#"<link rel="shortcut icon" href="/favicon.ico">"#;
        let result = parse_icon_link(html, "https://example.com");
        assert_eq!(result, Some("https://example.com/favicon.ico".to_string()));
    }

    #[test]
    fn parse_icon_link_no_icon() {
        let html = r#"<link rel="stylesheet" href="/style.css">"#;
        assert_eq!(parse_icon_link(html, "https://example.com"), None);
    }

    #[test]
    fn parse_icon_link_absolute_href() {
        let html = r#"<link rel="icon" href="https://cdn.example.com/icon.png">"#;
        let result = parse_icon_link(html, "https://example.com");
        assert_eq!(result, Some("https://cdn.example.com/icon.png".to_string()));
    }

    #[test]
    fn parse_icon_link_protocol_relative() {
        let html = r#"<link rel="icon" href="//cdn.example.com/icon.png">"#;
        let result = parse_icon_link(html, "https://example.com");
        assert_eq!(
            result,
            Some("https://cdn.example.com/icon.png".to_string())
        );
    }

    #[test]
    fn parse_icon_link_case_insensitive() {
        let html = r#"<LINK REL="ICON" HREF="/icon.png">"#;
        let result = parse_icon_link(html, "https://example.com");
        assert_eq!(result, Some("https://example.com/icon.png".to_string()));
    }

    #[test]
    fn parse_icon_link_skips_non_icon_link_tags() {
        let html = r#"
            <link rel="stylesheet" href="/style.css">
            <link rel="icon" href="/real-icon.png">
        "#;
        let result = parse_icon_link(html, "https://example.com");
        assert_eq!(
            result,
            Some("https://example.com/real-icon.png".to_string())
        );
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
