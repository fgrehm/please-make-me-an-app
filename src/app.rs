use crate::config::AppConfig;
use crate::{adblock, icon, inject, notification, profile, tray};
use anyhow::Result;
use std::path::Path;
use std::time::{Duration, Instant};
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoop};
use tao::window::WindowBuilder;
use wry::{NewWindowResponse, WebContext, WebViewBuilder};

pub fn run(
    config: &AppConfig,
    profile_name: &str,
    data_dir: &Path,
    config_dir: &Path,
    debug: bool,
    url: &str,
) -> Result<()> {
    let event_loop = EventLoop::new();

    let display_title = if config.profiles.is_empty() {
        config.window.title.clone()
    } else {
        format!("{} ({})", config.window.title, profile_name)
    };

    let saved_state = if config.window.remember_position {
        profile::load_window_state(data_dir)
    } else {
        None
    };

    let mut window_builder = WindowBuilder::new()
        .with_title(&display_title)
        .with_min_inner_size(tao::dpi::LogicalSize::new(200u32, 200u32));

    if let Some(ref state) = saved_state {
        window_builder = window_builder
            .with_inner_size(tao::dpi::PhysicalSize::new(state.width, state.height))
            .with_position(tao::dpi::PhysicalPosition::new(state.x, state.y));
    } else {
        window_builder = window_builder.with_inner_size(tao::dpi::LogicalSize::new(
            config.window.width,
            config.window.height,
        ));
    }

    let window = window_builder.build(&event_loop)?;

    let webview_data_dir = data_dir.join("webview-data");
    let mut web_context = WebContext::new(Some(webview_data_dir));

    let mut builder = WebViewBuilder::new_with_web_context(&mut web_context)
        .with_url(url)
        .with_clipboard(config.clipboard);

    if let Some(ua) = &config.user_agent {
        builder = builder.with_user_agent(ua);
        // WebKitGTK ignores with_user_agent for navigator.userAgent in JS,
        // so also override it via initialization script
        builder = builder.with_initialization_script(build_ua_override_script(ua));
    }

    if let Some(script) = build_navigator_override_script(&config.navigator) {
        builder = builder.with_initialization_script(&script);
    }

    if let Some(script) = inject::build_init_script(config, config_dir)? {
        builder = builder.with_initialization_script(&script);
    }

    if debug {
        builder = builder.with_initialization_script(
            r#"(function() {
    function dbg(msg) { window.ipc.postMessage('pmma-debug:' + msg); }
    dbg('[debug] userAgent: ' + navigator.userAgent);
    dbg('[debug] vendor: ' + navigator.vendor);
    dbg('[debug] platform: ' + navigator.platform);
    dbg('[debug] window.chrome: ' + typeof window.chrome + (window.chrome ? ' ' + JSON.stringify(window.chrome) : ''));
    dbg('[debug] location: ' + location.href);
    // Also log on page load in case of redirects
    window.addEventListener('load', function() {
        dbg('[debug] loaded location: ' + location.href);
        dbg('[debug] loaded userAgent: ' + navigator.userAgent);
        dbg('[debug] loaded vendor: ' + navigator.vendor);
        dbg('[debug] loaded platform: ' + navigator.platform);
    });
})();"#,
        );
    }

    if config.adblock {
        let script = adblock::build_script(config.adblock_extra.as_deref(), config_dir);
        builder = builder.with_initialization_script(&script);
    }

    let icon_path = icon::cached_path(&config.name);

    if config.notifications {
        builder = builder.with_initialization_script(notification::intercept_script());
        builder = builder.with_initialization_script(notification::dialog_intercept_script());
    }

    let needs_ipc = config.notifications || debug;
    if needs_ipc {
        // Cloned into the IPC closure which must be 'static (outlives this function)
        let app_name = config.name.clone();
        let ipc_icon_path = icon_path.clone();
        let ipc_notifications = config.notifications;
        builder = builder.with_ipc_handler(move |req: wry::http::Request<String>| {
            let body = req.body();
            if let Some(msg) = body.strip_prefix("pmma-debug:") {
                eprintln!("{}", msg);
            } else if ipc_notifications {
                notification::handle_ipc(body, &app_name, ipc_icon_path.as_deref());
            }
        });
    }

    let app_domain = extract_domain(&config.url).unwrap_or("").to_string();
    let allowed = config.allowed_domains.clone();
    let nav_debug = debug;
    let nav_open_external = config.open_external_links;
    builder = builder.with_navigation_handler(move |url| {
        if should_open_externally(&url, &app_domain, &allowed) {
            if nav_debug {
                eprintln!("[debug] nav blocked (external): {}", url);
            }
            if nav_open_external {
                open_in_browser(&url);
            }
            false
        } else {
            if nav_debug {
                eprintln!("[debug] nav allowed: {}", url);
            }
            true
        }
    });

    let popup_debug = debug;
    builder = builder.with_new_window_req_handler(move |url, _features| {
        // Never allow popups: NewWindowResponse::Allow creates unmanaged GTK windows
        // with no icon, no navigation handler, and no tray integration.
        // Instead, open in the system browser (which has password managers, WebAuthn, etc.).
        if url.starts_with("http://") || url.starts_with("https://") {
            let target = unwrap_google_redirect(&url).unwrap_or(url);
            if popup_debug {
                eprintln!("[debug] popup -> browser: {}", target);
            }
            open_in_browser(&target);
        } else if popup_debug {
            eprintln!("[debug] popup denied (non-http): {}", url);
        }
        NewWindowResponse::Deny
    });

    #[cfg(target_os = "linux")]
    let _webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        let vbox = window
            .default_vbox()
            .expect("GTK windows always have a default vbox on Linux");
        builder.build_gtk(vbox)?
    };

    #[cfg(not(target_os = "linux"))]
    let _webview = builder.build(&window)?;

    // Decode icon once, share between window and tray
    let icon_rgba = icon_path.as_deref().and_then(icon::load_rgba);

    // Window icon for alt-tab / taskbar
    if let Some((ref rgba, width, height)) = icon_rgba {
        if let Ok(win_icon) = tao::window::Icon::from_rgba(rgba.clone(), width, height) {
            window.set_window_icon(Some(win_icon));
        }
    }

    // System tray
    let tray_state = if config.tray.enabled {
        Some(tray::create(&display_title, icon_rgba)?)
    } else {
        None
    };

    let minimize_to_tray = config.tray.enabled && config.tray.minimize_to_tray;
    let remember_position = config.window.remember_position;
    let data_dir = data_dir.to_path_buf();
    let has_tray = tray_state.is_some();
    let mut window_visible = true;
    event_loop.run(move |event, _, control_flow| {
        // Tray menu events arrive via a separate channel, not as tao events.
        // Poll periodically so we actually receive them.
        if has_tray {
            *control_flow = ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(250));
        } else {
            *control_flow = ControlFlow::Wait;
        }

        // Keep webview alive
        let _ = &_webview;

        // Handle tray events
        if let Some(ref ts) = tray_state {
            if let Ok(menu_event) = tray_icon::menu::MenuEvent::receiver().try_recv() {
                if menu_event.id == ts.quit_id {
                    save_window_position(&window, remember_position, &data_dir);
                    *control_flow = ControlFlow::Exit;
                } else if menu_event.id == ts.toggle_id {
                    window_visible = !window_visible;
                    toggle_window(&window, window_visible);
                }
            }

            if let Ok(tray_icon::TrayIconEvent::Click {
                button: tray_icon::MouseButton::Left,
                ..
            }) = tray_icon::TrayIconEvent::receiver().try_recv()
            {
                window_visible = !window_visible;
                toggle_window(&window, window_visible);
            }
        }

        if let Event::WindowEvent {
            event: WindowEvent::CloseRequested,
            ..
        } = event
        {
            if minimize_to_tray {
                window_visible = false;
                window.set_visible(false);
            } else {
                save_window_position(&window, remember_position, &data_dir);
                *control_flow = ControlFlow::Exit;
            }
        }
    });
}

fn save_window_position(window: &tao::window::Window, enabled: bool, data_dir: &Path) {
    if !enabled {
        return;
    }
    if let Ok(pos) = window.outer_position() {
        let size = window.inner_size();
        profile::save_window_state(
            data_dir,
            &profile::WindowState {
                x: pos.x,
                y: pos.y,
                width: size.width,
                height: size.height,
            },
        );
    }
}

/// Show or hide the window. On Linux, use gtk_window_present() after showing
/// to properly re-activate the window in the compositor. Without this, KDE
/// Plasma leaves the titlebar buttons (minimize, maximize, close) disabled.
fn toggle_window(window: &tao::window::Window, visible: bool) {
    if visible {
        window.set_visible(true);
        #[cfg(target_os = "linux")]
        {
            use gtk::prelude::GtkWindowExt as _;
            use tao::platform::unix::WindowExtUnix;
            window.gtk_window().present();
        }
    } else {
        window.set_visible(false);
    }
}

fn escape_js_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\'', "\\'")
}

/// Build a JS snippet that overrides navigator.userAgent.
/// WebKitGTK ignores wry's with_user_agent for the JS-visible property,
/// so we patch it via Object.defineProperty.
fn build_ua_override_script(ua: &str) -> String {
    format!(
        "Object.defineProperty(navigator, 'userAgent', {{ get: function() {{ return '{}'; }} }});",
        escape_js_string(ua)
    )
}

/// Build JS snippets to override navigator.vendor, navigator.platform, and/or window.chrome.
/// Returns None if no navigator fields are set in config.
fn build_navigator_override_script(nav: &crate::config::NavigatorConfig) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(vendor) = &nav.vendor {
        parts.push(format!(
            "Object.defineProperty(navigator, 'vendor', {{ get: function() {{ return '{}'; }} }});",
            escape_js_string(vendor)
        ));
    }
    if let Some(platform) = &nav.platform {
        parts.push(format!(
            "Object.defineProperty(navigator, 'platform', {{ get: function() {{ return '{}'; }} }});",
            escape_js_string(platform)
        ));
    }
    if nav.chrome {
        parts.push("window.chrome = { runtime: {} };".to_string());
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n"))
    }
}

/// Check if a URL should be opened in the system browser instead of the webview.
fn should_open_externally(url: &str, app_domain: &str, allowed_domains: &[String]) -> bool {
    // mailto: and tel: always go to the system handler
    if url.starts_with("mailto:") || url.starts_with("tel:") {
        return true;
    }

    // Non-HTTP schemes (about:, blob:, data:, javascript:) stay in the webview
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return false;
    }

    let domain = match extract_domain(url) {
        Some(d) => d,
        None => return false,
    };

    !domain_matches(domain, app_domain)
        && !allowed_domains
            .iter()
            .any(|d| domain_matches(domain, d))
}

fn domain_matches(domain: &str, pattern: &str) -> bool {
    domain == pattern
        || (domain.len() > pattern.len()
            && domain.ends_with(pattern)
            && domain.as_bytes()[domain.len() - pattern.len() - 1] == b'.')
}

fn extract_domain(url: &str) -> Option<&str> {
    let after_scheme = url.find("://").map(|i| &url[i + 3..])?;
    let host = after_scheme.split('/').next()?;
    Some(host.split(':').next().unwrap_or(host))
}

/// If the URL is a Google redirect (google.com/url?q=<encoded-url>), extract the destination.
fn unwrap_google_redirect(url: &str) -> Option<String> {
    let domain = extract_domain(url)?;
    if !domain_matches(domain, "google.com") {
        return None;
    }
    let after_scheme = url.find("://").map(|i| &url[i + 3..])?;
    let slash = after_scheme.find('/')?;
    let path_and_query = &after_scheme[slash..];
    if !path_and_query.starts_with("/url?") {
        return None;
    }
    let query = path_and_query.split('?').nth(1)?;
    for pair in query.split('&') {
        if let Some(value) = pair.strip_prefix("q=") {
            let decoded = percent_decode(value);
            if decoded.starts_with("http://") || decoded.starts_with("https://") {
                return Some(decoded);
            }
        }
    }
    None
}

fn percent_decode(s: &str) -> String {
    let mut result = Vec::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) =
                u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16)
            {
                result.push(byte);
                i += 3;
                continue;
            }
        }
        result.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&result).into_owned()
}

fn open_in_browser(url: &str) {
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open")
            .arg(url)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }

    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }

    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/c", "start", "", url])
            .spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_domain_stays_in_app() {
        assert!(!should_open_externally(
            "https://mail.google.com/inbox",
            "mail.google.com",
            &[]
        ));
    }

    #[test]
    fn subdomain_of_app_stays_in_app() {
        assert!(!should_open_externally(
            "https://sub.mail.google.com/page",
            "mail.google.com",
            &[]
        ));
    }

    #[test]
    fn different_domain_opens_externally() {
        assert!(should_open_externally(
            "https://example.com/link",
            "mail.google.com",
            &[]
        ));
    }

    #[test]
    fn allowed_domain_stays_in_app() {
        let allowed = vec!["accounts.google.com".to_string()];
        assert!(!should_open_externally(
            "https://accounts.google.com/login",
            "mail.google.com",
            &allowed
        ));
    }

    #[test]
    fn allowed_parent_domain_matches_subdomains() {
        let allowed = vec!["google.com".to_string()];
        assert!(!should_open_externally(
            "https://accounts.google.com/login",
            "mail.google.com",
            &allowed
        ));
    }

    #[test]
    fn mailto_opens_externally() {
        assert!(should_open_externally(
            "mailto:user@example.com",
            "mail.google.com",
            &[]
        ));
    }

    #[test]
    fn tel_opens_externally() {
        assert!(should_open_externally(
            "tel:+1234567890",
            "mail.google.com",
            &[]
        ));
    }

    #[test]
    fn blob_stays_in_app() {
        assert!(!should_open_externally(
            "blob:https://mail.google.com/abc123",
            "mail.google.com",
            &[]
        ));
    }

    #[test]
    fn about_blank_stays_in_app() {
        assert!(!should_open_externally(
            "about:blank",
            "mail.google.com",
            &[]
        ));
    }

    #[test]
    fn extract_domain_basic() {
        assert_eq!(
            extract_domain("https://mail.google.com/inbox"),
            Some("mail.google.com")
        );
    }

    #[test]
    fn extract_domain_with_port() {
        assert_eq!(
            extract_domain("http://localhost:3000/page"),
            Some("localhost")
        );
    }

    #[test]
    fn extract_domain_no_path() {
        assert_eq!(
            extract_domain("https://example.com"),
            Some("example.com")
        );
    }

    // -- domain_matches --

    #[test]
    fn domain_matches_exact() {
        assert!(domain_matches("example.com", "example.com"));
    }

    #[test]
    fn domain_matches_subdomain() {
        assert!(domain_matches("sub.example.com", "example.com"));
    }

    #[test]
    fn domain_matches_no_partial() {
        // "notexample.com" should NOT match "example.com"
        assert!(!domain_matches("notexample.com", "example.com"));
    }

    #[test]
    fn domain_matches_empty_pattern() {
        assert!(!domain_matches("example.com", ""));
    }

    // -- build_ua_override_script --

    #[test]
    fn ua_override_contains_user_agent() {
        let script = build_ua_override_script("Mozilla/5.0 Test");
        assert!(script.contains("Mozilla/5.0 Test"));
        assert!(script.contains("navigator"));
        assert!(script.contains("userAgent"));
    }

    #[test]
    fn ua_override_escapes_quotes() {
        let script = build_ua_override_script("it's a test");
        assert!(script.contains(r"it\'s a test"));
    }

    #[test]
    fn ua_override_escapes_backslashes() {
        let script = build_ua_override_script(r"back\slash");
        assert!(script.contains(r"back\\slash"));
    }

    // -- build_navigator_override_script --

    #[test]
    fn navigator_override_none_when_empty() {
        let nav = crate::config::NavigatorConfig { vendor: None, platform: None, chrome: false };
        assert!(build_navigator_override_script(&nav).is_none());
    }

    #[test]
    fn navigator_override_vendor_only() {
        let nav = crate::config::NavigatorConfig {
            vendor: Some("Google Inc.".to_string()),
            platform: None,
            chrome: false,
        };
        let script = build_navigator_override_script(&nav).unwrap();
        assert!(script.contains("vendor"));
        assert!(script.contains("Google Inc."));
        assert!(!script.contains("platform"));
    }

    #[test]
    fn navigator_override_platform_only() {
        let nav = crate::config::NavigatorConfig {
            vendor: None,
            platform: Some("Linux x86_64".to_string()),
            chrome: false,
        };
        let script = build_navigator_override_script(&nav).unwrap();
        assert!(script.contains("platform"));
        assert!(script.contains("Linux x86_64"));
        assert!(!script.contains("vendor"));
    }

    #[test]
    fn navigator_override_both() {
        let nav = crate::config::NavigatorConfig {
            vendor: Some("Google Inc.".to_string()),
            platform: Some("Linux x86_64".to_string()),
            chrome: false,
        };
        let script = build_navigator_override_script(&nav).unwrap();
        assert!(script.contains("vendor"));
        assert!(script.contains("Google Inc."));
        assert!(script.contains("platform"));
        assert!(script.contains("Linux x86_64"));
    }

    #[test]
    fn navigator_override_escapes_quotes() {
        let nav = crate::config::NavigatorConfig {
            vendor: Some("it's inc".to_string()),
            platform: None,
            chrome: false,
        };
        let script = build_navigator_override_script(&nav).unwrap();
        assert!(script.contains(r"it\'s inc"));
    }

    #[test]
    fn navigator_override_empty_string_vendor() {
        // Empty string is valid (e.g. Firefox reports empty vendor)
        let nav = crate::config::NavigatorConfig {
            vendor: Some(String::new()),
            platform: None,
            chrome: false,
        };
        let script = build_navigator_override_script(&nav).unwrap();
        assert!(script.contains("vendor"));
    }

    #[test]
    fn navigator_override_chrome_flag() {
        let nav = crate::config::NavigatorConfig {
            vendor: None,
            platform: None,
            chrome: true,
        };
        let script = build_navigator_override_script(&nav).unwrap();
        assert!(script.contains("window.chrome"));
        assert!(script.contains("runtime"));
    }

    #[test]
    fn navigator_override_chrome_false_still_none_when_no_others() {
        let nav = crate::config::NavigatorConfig { vendor: None, platform: None, chrome: false };
        assert!(build_navigator_override_script(&nav).is_none());
    }

    // -- unwrap_google_redirect --

    #[test]
    fn unwrap_google_redirect_basic() {
        let url = "https://www.google.com/url?q=https%3A%2F%2Fexample.com%2Fpage&sa=D&ust=123";
        assert_eq!(
            unwrap_google_redirect(url),
            Some("https://example.com/page".to_string())
        );
    }

    #[test]
    fn unwrap_google_redirect_no_q_param() {
        let url = "https://www.google.com/url?sa=D&ust=123";
        assert_eq!(unwrap_google_redirect(url), None);
    }

    #[test]
    fn unwrap_google_redirect_not_google() {
        let url = "https://example.com/url?q=https%3A%2F%2Fother.com";
        assert_eq!(unwrap_google_redirect(url), None);
    }

    #[test]
    fn unwrap_google_redirect_not_url_path() {
        let url = "https://www.google.com/search?q=test";
        assert_eq!(unwrap_google_redirect(url), None);
    }

    #[test]
    fn unwrap_google_redirect_rejects_non_http() {
        let url = "https://www.google.com/url?q=javascript%3Aalert(1)";
        assert_eq!(unwrap_google_redirect(url), None);
    }

    // -- percent_decode --

    #[test]
    fn percent_decode_basic() {
        assert_eq!(percent_decode("hello%20world"), "hello world");
    }

    #[test]
    fn percent_decode_url() {
        assert_eq!(
            percent_decode("https%3A%2F%2Fexample.com%2Fpage%3Fk%3Dv"),
            "https://example.com/page?k=v"
        );
    }

    #[test]
    fn percent_decode_no_encoding() {
        assert_eq!(percent_decode("plain-text"), "plain-text");
    }

    #[test]
    fn percent_decode_invalid_hex() {
        assert_eq!(percent_decode("100%ZZdone"), "100%ZZdone");
    }

    #[test]
    fn percent_decode_truncated() {
        assert_eq!(percent_decode("trail%2"), "trail%2");
    }
}
