use crate::config::AppConfig;
use crate::{adblock, icon, inject, notification, profile, tray};
use anyhow::Result;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoop};
use tao::window::{Fullscreen, WindowBuilder};
use wry::{NewWindowResponse, WebContext, WebViewBuilder};

pub fn run(
    config: &AppConfig,
    profile_name: &str,
    data_dir: &Path,
    config_dir: &Path,
    debug: bool,
    url: &str,
) -> Result<()> {
    // Set app-id to match the .desktop file name so the compositor can find
    // the correct icon for alt-tab, taskbar, etc. On Wayland, GTK uses
    // g_get_prgname() as the xdg_toplevel app_id.
    let app_id = if config.profiles.is_empty() {
        format!("pmma-{}", config.name)
    } else {
        format!("pmma-{}--{}", config.name, profile_name)
    };
    #[cfg(target_os = "linux")]
    gtk::glib::set_prgname(Some(&app_id));

    // Bind the raise socket early (before window/webview setup) so that a second
    // instance signalling raise does not miss the socket. The flock is already held
    // by the caller (main). The thread that reads from the socket is spawned below.
    let raise_listener = profile::create_raise_listener(data_dir);

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

    // WebKitGTK does not populate clipboardData.items with image data on paste
    // events, but the async Clipboard API (navigator.clipboard.read()) works.
    // This polyfill intercepts paste events, reads image data via the async API,
    // and re-dispatches a synthetic paste event with the image blob attached so
    // web apps that expect clipboardData.items (like WhatsApp Web) can handle it.
    if config.clipboard {
        builder = builder.with_initialization_script(clipboard_image_paste_polyfill());
    }

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

    let blocked_domains = if config.adblock {
        let script = adblock::build_script(config.adblock_extra.as_deref(), config_dir);
        builder = builder.with_initialization_script(&script);
        Some(adblock::load_blocked_domains(
            config.adblock_extra.as_deref(),
            config_dir,
        ))
    } else {
        None
    };

    let icon_path = icon::cached_path(&config.name);

    if config.notifications {
        builder = builder.with_initialization_script(notification::intercept_script());
        builder = builder.with_initialization_script(notification::dialog_intercept_script());
    }

    // Per-launch IPC token: injected into init scripts and validated in the IPC
    // handler so that only our scripts (not arbitrary page JS) can trigger host
    // actions like quit, close, or beforeunload confirmation.
    let ipc_token: u64 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
        ^ std::process::id() as u64;
    let ipc_token = format!("{:x}", ipc_token);
    builder = builder.with_initialization_script(format!(
        "Object.defineProperty(window, '__pmma_token', {{value: '{}', configurable: false, writable: false}});",
        ipc_token
    ));

    // Keyboard shortcuts (Ctrl+W to close window, Ctrl+Q to quit)
    builder = builder.with_initialization_script(keyboard_shortcut_script());

    // Fullscreen API polyfill: maps requestFullscreen/exitFullscreen to native
    // window fullscreen via IPC. Without this, video fullscreen buttons are no-ops
    // because WebKitGTK's built-in fullscreen implementation is not wired up.
    builder = builder.with_initialization_script(fullscreen_polyfill_script());

    // Flags shared between the IPC handler and the event loop.
    // The IPC handler sets them; the event loop reads and resets them.
    let raise_requested = Arc::new(AtomicBool::new(false));
    let close_requested = Arc::new(AtomicBool::new(false));
    let quit_requested = Arc::new(AtomicBool::new(false));
    let close_confirmed = Arc::new(AtomicBool::new(false));
    let close_blocked = Arc::new(AtomicBool::new(false));
    let go_back_requested = Arc::new(AtomicBool::new(false));
    let go_forward_requested = Arc::new(AtomicBool::new(false));
    let reload_requested = Arc::new(AtomicBool::new(false));
    let reload_hard_requested = Arc::new(AtomicBool::new(false));
    let show_url_requested = Arc::new(AtomicBool::new(false));
    let enter_fullscreen_requested = Arc::new(AtomicBool::new(false));
    let exit_fullscreen_requested = Arc::new(AtomicBool::new(false));

    {
        // Cloned into the IPC closure which must be 'static (outlives this function)
        let app_name = config.name.clone();
        let ipc_icon_path = icon_path.clone();
        let ipc_notifications = config.notifications;
        let ipc_raise = raise_requested.clone();
        let ipc_close = close_requested.clone();
        let ipc_quit = quit_requested.clone();
        let ipc_confirmed = close_confirmed.clone();
        let ipc_blocked = close_blocked.clone();
        let ipc_go_back = go_back_requested.clone();
        let ipc_go_forward = go_forward_requested.clone();
        let ipc_reload = reload_requested.clone();
        let ipc_reload_hard = reload_hard_requested.clone();
        let ipc_show_url = show_url_requested.clone();
        let ipc_enter_fs = enter_fullscreen_requested.clone();
        let ipc_exit_fs = exit_fullscreen_requested.clone();
        let ipc_token_prefix = format!("{}:", ipc_token);
        builder = builder.with_ipc_handler(move |req: wry::http::Request<String>| {
            let body = req.body();
            if let Some(msg) = body.strip_prefix("pmma-debug:") {
                eprintln!("{}", msg);
            } else if let Some(cmd) = body.strip_prefix(&ipc_token_prefix) {
                // Token-authenticated host control messages
                match cmd {
                    "pmma-kbd:close-window" => ipc_close.store(true, Ordering::Release),
                    "pmma-kbd:quit-app" => ipc_quit.store(true, Ordering::Release),
                    "pmma-close:confirmed" => ipc_confirmed.store(true, Ordering::Release),
                    "pmma-close:blocked" => ipc_blocked.store(true, Ordering::Release),
                    "pmma-kbd:go-back" => ipc_go_back.store(true, Ordering::Release),
                    "pmma-kbd:go-forward" => ipc_go_forward.store(true, Ordering::Release),
                    "pmma-kbd:reload" => ipc_reload.store(true, Ordering::Release),
                    "pmma-kbd:reload-hard" => ipc_reload_hard.store(true, Ordering::Release),
                    "pmma-kbd:show-url" => ipc_show_url.store(true, Ordering::Release),
                    "pmma-fullscreen:enter" => ipc_enter_fs.store(true, Ordering::Release),
                    "pmma-fullscreen:exit" => ipc_exit_fs.store(true, Ordering::Release),
                    _ => {}
                }
            } else if ipc_notifications {
                notification::handle_ipc(
                    body,
                    &app_name,
                    ipc_icon_path.as_deref(),
                    Some(&ipc_raise),
                );
            }
        });
    }

    let app_domain = extract_domain(&config.url).unwrap_or("").to_string();
    let allowed = config.allowed_domains.clone();
    let excluded = config.excluded_domains.clone();
    let url_dialog_app_domain = app_domain.clone();
    let url_dialog_allowed = allowed.clone();
    let url_dialog_excluded = excluded.clone();
    let nav_debug = debug;
    let nav_open_external = config.open_external_links;
    builder = builder.with_navigation_handler(move |url| {
        if should_open_externally(&url, &app_domain, &allowed, &excluded) {
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
    let (popup_tx, popup_rx) = mpsc::channel::<String>();
    builder = builder.with_new_window_req_handler(move |url, _features| {
        // Never allow popups: NewWindowResponse::Allow creates unmanaged GTK windows
        // with no icon, no navigation handler, and no tray integration.
        // Instead, open in the system browser (which has password managers, WebAuthn, etc.).
        if url.starts_with("http://") || url.starts_with("https://") {
            // Silently deny popups from ad/tracker domains when adblock is enabled.
            if let Some(ref domains) = blocked_domains
                && adblock::is_blocked(domains, &url) {
                    if popup_debug {
                        eprintln!("[debug] popup blocked (adblock): {}", url);
                    }
                    return NewWindowResponse::Deny;
                }
            let target = unwrap_google_redirect(&url).unwrap_or(url);
            if popup_debug {
                eprintln!("[debug] popup -> browser: {}", target);
            }
            let _ = popup_tx.send(format!("Opened in browser: {}", target));
            open_in_browser(&target);
        } else {
            if popup_debug {
                eprintln!("[debug] popup denied (non-http): {}", url);
            }
            let _ = popup_tx.send(format!("Popup denied: {}", url));
        }
        NewWindowResponse::Deny
    });

    builder = builder.with_download_started_handler(move |_url, dest| {
        match pick_download_path(dest) {
            Some(chosen) => {
                *dest = chosen;
                true
            }
            // User cancelled the dialog: deny the download rather than
            // silently saving to the default location.
            None => false,
        }
    });

    #[cfg(target_os = "linux")]
    let webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        let vbox = window
            .default_vbox()
            .expect("GTK windows always have a default vbox on Linux");
        builder.build_gtk(vbox)?
    };

    #[cfg(not(target_os = "linux"))]
    let webview = builder.build(&window)?;

    // Decode icon once, share between window and tray
    let icon_rgba = icon_path.as_deref().and_then(icon::load_rgba);

    // Window icon for alt-tab / taskbar
    if let Some((ref rgba, width, height)) = icon_rgba
        && let Ok(win_icon) = tao::window::Icon::from_rgba(rgba.clone(), width, height) {
            window.set_window_icon(Some(win_icon));
        }

    // System tray
    let tray_state = if config.tray.enabled {
        Some(tray::create(&display_title, icon_rgba)?)
    } else {
        None
    };

    // Spawn the raise listener thread now that raise_requested is available.
    match raise_listener {
        Ok(listener) => {
            let listener_raise = raise_requested.clone();
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    match stream {
                        Ok(_) => listener_raise.store(true, Ordering::Release),
                        Err(_) => break,
                    }
                }
            });
        }
        Err(e) => {
            if debug {
                eprintln!("[debug] raise listener failed (raise-existing-window disabled): {}", e);
            }
        }
    }

    let minimize_to_tray = config.tray.enabled && config.tray.minimize_to_tray;
    let mut is_fullscreen = false;
    let remember_position = config.window.remember_position;
    let data_dir = data_dir.to_path_buf();
    let mut window_visible = true;
    let mut close_pending = false;
    event_loop.run(move |event, _, control_flow| {
        // Always poll at 250ms. Keyboard shortcuts (always injected), the raise
        // socket listener thread, and tray menu events all signal via AtomicBool
        // or a separate channel -- none of them generate tao window events, so
        // WaitUntil is required to ensure they are checked in a timely manner.
        *control_flow = ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(250));

        // beforeunload check confirmed: safe to close
        if close_confirmed.swap(false, Ordering::Acquire) {
            save_window_position(&window, remember_position, &data_dir);
            fire_page_cleanup(&webview);
            std::process::exit(0);
        }

        // beforeunload blocked the close: ask the user
        if close_blocked.swap(false, Ordering::Acquire) {
            close_pending = false;
            if show_close_confirmation(&window) {
                save_window_position(&window, remember_position, &data_dir);
                fire_page_cleanup(&webview);
                std::process::exit(0);
            }
            return;
        }

        // Raise window if requested (notification click, tray click, or another instance)
        if raise_requested.swap(false, Ordering::Acquire) {
            window_visible = true;
            toggle_window(&window, true);
        }

        // Ctrl+Q: quit regardless of tray
        if quit_requested.swap(false, Ordering::Acquire) && !close_pending {
            close_pending = true;
            if webview.evaluate_script(BEFOREUNLOAD_CHECK).is_err() {
                save_window_position(&window, remember_position, &data_dir);
                fire_page_cleanup(&webview);
                std::process::exit(0);
            }
            return;
        }

        // Ctrl+W: same as X button (hide to tray if enabled, otherwise close)
        if close_requested.swap(false, Ordering::Acquire) && !close_pending {
            if minimize_to_tray {
                window_visible = false;
                toggle_window(&window, false);
            } else {
                close_pending = true;
                if webview.evaluate_script(BEFOREUNLOAD_CHECK).is_err() {
                    save_window_position(&window, remember_position, &data_dir);
                    fire_page_cleanup(&webview);
                    std::process::exit(0);
                }
            }
            return;
        }

        if go_back_requested.swap(false, Ordering::Acquire) {
            let _ = webview.evaluate_script("window.history.back()");
        }

        if go_forward_requested.swap(false, Ordering::Acquire) {
            let _ = webview.evaluate_script("window.history.forward()");
        }

        if reload_requested.swap(false, Ordering::Acquire) {
            let _ = webview.reload();
        }

        // Ctrl+Shift+R: hard reload bypassing cache. WebKit honors the
        // deprecated `true` argument to location.reload(); wry has no
        // dedicated API for this.
        if reload_hard_requested.swap(false, Ordering::Acquire) {
            let _ = webview.evaluate_script("location.reload(true)");
        }

        if show_url_requested.swap(false, Ordering::Acquire)
            && let Ok(url) = webview.url()
                && let Some(new_url) = show_url_dialog(
                    &window,
                    &url,
                    &url_dialog_app_domain,
                    &url_dialog_allowed,
                    &url_dialog_excluded,
                ) {
                    let _ = webview.load_url(&new_url);
                }

        if enter_fullscreen_requested.swap(false, Ordering::Acquire) {
            is_fullscreen = true;
            window.set_fullscreen(Some(Fullscreen::Borderless(None)));
        }

        if exit_fullscreen_requested.swap(false, Ordering::Acquire) {
            is_fullscreen = false;
            window.set_fullscreen(None);
        }

        // Detect external fullscreen exits (compositor shortcut, WM, etc.).
        // If we think we're fullscreen but the window disagrees, reset JS state.
        if is_fullscreen && window.fullscreen().is_none() {
            is_fullscreen = false;
            let _ = webview
                .evaluate_script("if(window.__pmma_fs_reset)window.__pmma_fs_reset()");
        }

        // Show toast for any popup that was sent to the browser (or denied)
        while let Ok(msg) = popup_rx.try_recv() {
            let _ = webview.evaluate_script(&popup_toast_script(&msg));
        }

        // Handle tray events
        if let Some(ref ts) = tray_state {
            if let Ok(menu_event) = tray_icon::menu::MenuEvent::receiver().try_recv() {
                if menu_event.id == ts.quit_id {
                    save_window_position(&window, remember_position, &data_dir);
                    fire_page_cleanup(&webview);
                    std::process::exit(0);
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
                toggle_window(&window, false);
            } else if !close_pending {
                close_pending = true;
                if webview.evaluate_script(BEFOREUNLOAD_CHECK).is_err() {
                    save_window_position(&window, remember_position, &data_dir);
                    fire_page_cleanup(&webview);
                    std::process::exit(0);
                }
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

/// Show or hide the window for tray minimize/restore.
///
/// On Wayland, hiding a window unmaps the xdg_toplevel surface and the
/// compositor resets all window capabilities (minimize, maximize, close).
/// When remapped via show_all(), KDE Plasma does not restore these
/// capabilities, leaving titlebar buttons disabled. Forcing a resize after
/// showing triggers a new configure event from the compositor that restores
/// the capabilities. A short timer resizes back to the original size.
/// Fire pagehide + unload in the webview and wait briefly for synchronous
/// handlers to complete. Called before every process exit so apps that track
/// open tabs via these events get a chance to clean up.
fn fire_page_cleanup(webview: &wry::WebView) {
    let _ = webview.evaluate_script(PAGE_CLEANUP);
    std::thread::sleep(Duration::from_millis(150));
}

fn toggle_window(window: &tao::window::Window, visible: bool) {
    #[cfg(target_os = "linux")]
    {
        use gtk::prelude::{GtkWindowExt as _, WidgetExt as _};
        use tao::platform::unix::WindowExtUnix;
        let gtk_win = window.gtk_window();
        if visible {
            let was_hidden = !gtk_win.is_visible();
            gtk_win.show_all();
            if was_hidden {
                // Wayland workaround: unmapping resets compositor state (titlebar
                // buttons). A 1px resize forces a configure event that restores them.
                let (w, h) = gtk_win.size();
                gtk_win.resize(w, h + 1);
                let win_ref = gtk_win.clone();
                gtk::glib::timeout_add_local_once(
                    std::time::Duration::from_millis(50),
                    move || {
                        win_ref.resize(w, h);
                    },
                );
            }
            gtk_win.present();
        } else {
            gtk_win.hide();
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        window.set_visible(visible);
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
fn should_open_externally(
    url: &str,
    app_domain: &str,
    allowed_domains: &[String],
    excluded_domains: &[String],
) -> bool {
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

    // Excluded domains always open externally, even if they match allowed_domains
    if excluded_domains.iter().any(|d| domain_matches(domain, d)) {
        return true;
    }

    !domain_matches(domain, app_domain)
        && !allowed_domains.iter().any(|d| domain_matches(domain, d))
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
        if bytes[i] == b'%' && i + 2 < bytes.len()
            && let Ok(byte) =
                u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16)
            {
                result.push(byte);
                i += 3;
                continue;
            }
        result.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&result).into_owned()
}

/// JS polyfill for image paste on WebKitGTK. The browser does not populate
/// `clipboardData.items` with image data on paste events, but the async
/// Clipboard API (`navigator.clipboard.read()`) works. This script intercepts
/// paste events in the capture phase, reads image blobs via the async API,
/// and re-dispatches a new ClipboardEvent with the data attached.
fn clipboard_image_paste_polyfill() -> &'static str {
    r#"(function() {
    if (!navigator.clipboard || !navigator.clipboard.read) return;
    var handling = false;
    document.addEventListener('paste', function(e) {
        if (handling) return;
        // If clipboardData already has items, the browser handled it natively
        if (e.clipboardData && e.clipboardData.items && e.clipboardData.items.length > 0) return;
        var target = e.target || document.body;
        // Read from the async Clipboard API and re-dispatch
        e.preventDefault();
        e.stopImmediatePropagation();
        navigator.clipboard.read().then(function(items) {
            if (!items.length) return;
            var dt = new DataTransfer();
            var pending = 0;
            function settle() {
                pending--;
                if (pending === 0 && dt.items.length > 0) {
                    handling = true;
                    target.dispatchEvent(new ClipboardEvent('paste', {
                        clipboardData: dt,
                        bubbles: true,
                        cancelable: true
                    }));
                    handling = false;
                }
            }
            items.forEach(function(item) {
                item.types.forEach(function(type) {
                    if (type.indexOf('image/') !== 0) return;
                    pending++;
                    item.getType(type).then(function(blob) {
                        dt.items.add(new File([blob], 'paste.' + type.split('/')[1], {type: type}));
                        settle();
                    }).catch(function() {
                        settle();
                    });
                });
            });
        }).catch(function() {});
    }, true);
})();"#
}

/// JS initialization script that intercepts Ctrl+W and Ctrl+Q keyboard
/// shortcuts and forwards them to the host via IPC. WebKitGTK consumes all
/// key events for webview content, so tao never sees them. Using the capture
/// phase ensures we intercept before the web app's own handlers.
fn keyboard_shortcut_script() -> &'static str {
    r#"document.addEventListener('keydown', function(e) {
    var t = window.__pmma_token || '';
    if ((e.ctrlKey || e.metaKey) && !e.shiftKey && !e.altKey) {
        if (e.key === 'w' || e.key === 'W') {
            e.preventDefault();
            e.stopPropagation();
            window.ipc.postMessage(t + ':pmma-kbd:close-window');
        } else if (e.key === 'q' || e.key === 'Q') {
            e.preventDefault();
            e.stopPropagation();
            window.ipc.postMessage(t + ':pmma-kbd:quit-app');
        } else if (e.key === 'r' || e.key === 'R') {
            e.preventDefault();
            e.stopPropagation();
            window.ipc.postMessage(t + ':pmma-kbd:reload');
        } else if (e.key === 'l' || e.key === 'L') {
            e.preventDefault();
            e.stopPropagation();
            window.ipc.postMessage(t + ':pmma-kbd:show-url');
        }
    } else if ((e.ctrlKey || e.metaKey) && e.shiftKey && !e.altKey) {
        if (e.key === 'r' || e.key === 'R') {
            e.preventDefault();
            e.stopPropagation();
            window.ipc.postMessage(t + ':pmma-kbd:reload-hard');
        }
    } else if (e.altKey && !e.ctrlKey && !e.metaKey && !e.shiftKey) {
        if (e.key === 'ArrowLeft') {
            e.preventDefault();
            e.stopPropagation();
            window.ipc.postMessage(t + ':pmma-kbd:go-back');
        } else if (e.key === 'ArrowRight') {
            e.preventDefault();
            e.stopPropagation();
            window.ipc.postMessage(t + ':pmma-kbd:go-forward');
        }
    }
}, true);"#
}

/// JS initialization script that polyfills the Fullscreen API.
///
/// WebKitGTK has its own fullscreen implementation but wry does not wire it up
/// to the host window, so `element.requestFullscreen()` is a no-op. This
/// polyfill intercepts all standard and webkit-prefixed fullscreen calls,
/// forwards them to the host via IPC, and dispatches `fullscreenchange` events
/// so web apps (video players, games, etc.) behave correctly.
fn fullscreen_polyfill_script() -> &'static str {
    r#"(function() {
    if (window.__pmma_fullscreen__) return;
    window.__pmma_fullscreen__ = true;

    var fsElement = null;

    function dispatch(el) {
        var ev = new Event('fullscreenchange', { bubbles: true });
        document.dispatchEvent(ev);
        if (el) el.dispatchEvent(ev);
        var wk = new Event('webkitfullscreenchange', { bubbles: true });
        document.dispatchEvent(wk);
        if (el) el.dispatchEvent(wk);
    }

    function enter(el) {
        fsElement = el;
        var t = window.__pmma_token || '';
        window.ipc.postMessage(t + ':pmma-fullscreen:enter');
        dispatch(el);
        return Promise.resolve();
    }

    function exit() {
        var prev = fsElement;
        fsElement = null;
        var t = window.__pmma_token || '';
        window.ipc.postMessage(t + ':pmma-fullscreen:exit');
        dispatch(prev);
        return Promise.resolve();
    }

    Object.defineProperty(document, 'fullscreenEnabled', { get: function() { return true; }, configurable: true });
    Object.defineProperty(document, 'webkitFullscreenEnabled', { get: function() { return true; }, configurable: true });
    Object.defineProperty(document, 'fullscreenElement', { get: function() { return fsElement; }, configurable: true });
    Object.defineProperty(document, 'webkitFullscreenElement', { get: function() { return fsElement; }, configurable: true });
    Object.defineProperty(document, 'webkitCurrentFullScreenElement', { get: function() { return fsElement; }, configurable: true });

    Element.prototype.requestFullscreen = function() { return enter(this); };
    Element.prototype.webkitRequestFullscreen = function() { return enter(this); };
    Element.prototype.webkitRequestFullScreen = function() { return enter(this); };

    document.exitFullscreen = exit;
    document.webkitExitFullscreen = exit;
    document.webkitCancelFullScreen = exit;

    // Escape exits fullscreen (mirrors native browser behaviour)
    document.addEventListener('keydown', function(e) {
        if (e.key === 'Escape' && fsElement) {
            e.preventDefault();
            exit();
        }
    }, true);

    // Called by the host when the window exits fullscreen externally
    // (compositor shortcut, WM, etc.) so JS state stays in sync.
    window.__pmma_fs_reset = function() {
        var prev = fsElement;
        fsElement = null;
        dispatch(prev);
    };
})();"#
}

/// JS evaluated on demand to check if the page's beforeunload handler would
/// block closing. Dispatches a synthetic beforeunload event and sends the
/// result back via IPC.
const BEFOREUNLOAD_CHECK: &str = r#"(function() {
    var t = window.__pmma_token || '';
    var event = new Event('beforeunload', { cancelable: true });
    // Generic Event.returnValue defaults to true (boolean), not "" like
    // BeforeUnloadEvent. Override so only explicit handler sets are detected.
    Object.defineProperty(event, 'returnValue', { value: '', writable: true });
    window.dispatchEvent(event);
    if (event.defaultPrevented || event.returnValue) {
        window.ipc.postMessage(t + ':pmma-close:blocked');
    } else {
        window.ipc.postMessage(t + ':pmma-close:confirmed');
    }
})();"#;

/// JS dispatched before process exit to fire page lifecycle cleanup events.
/// Gives pages a chance to decrement tab counters, flush localStorage state, etc.
/// Only synchronous handlers will complete; async cleanup won't survive process exit.
const PAGE_CLEANUP: &str = r#"(function() {
    try {
        window.dispatchEvent(new PageTransitionEvent('pagehide', { persisted: false }));
        window.dispatchEvent(new Event('unload'));
    } catch(e) {}
})();"#;

/// Show a GTK file chooser so the user can pick where to save a download.
/// Returns the chosen path, or None if the user cancelled.
/// The caller should return false from the download handler on None to cancel
/// the download rather than saving to the default location.
fn pick_download_path(suggested: &std::path::Path) -> Option<std::path::PathBuf> {
    #[cfg(target_os = "linux")]
    {
        use gtk::prelude::*;
        let dialog = gtk::FileChooserDialog::new(
            Some("Save file"),
            None::<&gtk::Window>,
            gtk::FileChooserAction::Save,
        );
        dialog.set_do_overwrite_confirmation(true);
        if let Some(name) = suggested.file_name() {
            dialog.set_current_name(&name.to_string_lossy());
        }
        // Use the XDG user-dirs download directory (reads ~/.config/user-dirs.dirs),
        // fall back to ~/Downloads, then /tmp.
        let downloads = directories::UserDirs::new()
            .and_then(|u| u.download_dir().map(|d| d.to_path_buf()))
            .unwrap_or_else(|| {
                std::env::var("HOME")
                    .map(|h| std::path::PathBuf::from(h).join("Downloads"))
                    .unwrap_or_else(|_| std::path::PathBuf::from("/tmp"))
            });
        dialog.set_current_folder(downloads);
        dialog.add_button("Cancel", gtk::ResponseType::Cancel);
        dialog.add_button("Save", gtk::ResponseType::Accept);
        let response = dialog.run();
        let chosen = if response == gtk::ResponseType::Accept {
            dialog.filename()
        } else {
            None
        };
        dialog.close();
        chosen
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = suggested;
        None
    }
}

/// Show a GTK confirmation dialog when beforeunload blocks the close.
/// Returns true if the user chose to leave, false if they chose to stay.
fn show_close_confirmation(window: &tao::window::Window) -> bool {
    #[cfg(target_os = "linux")]
    {
        use gtk::prelude::*;
        use tao::platform::unix::WindowExtUnix;
        let gtk_win = window.gtk_window();
        let parent: &gtk::Window = gtk_win.upcast_ref();
        let dialog = gtk::MessageDialog::new(
            Some(parent),
            gtk::DialogFlags::MODAL | gtk::DialogFlags::DESTROY_WITH_PARENT,
            gtk::MessageType::Question,
            gtk::ButtonsType::None,
            "Leave page?",
        );
        dialog.set_secondary_text(Some("Changes you made may not be saved."));
        dialog.add_button("Stay", gtk::ResponseType::Cancel);
        dialog.add_button("Leave", gtk::ResponseType::Accept);
        let response = dialog.run();
        dialog.close();
        response == gtk::ResponseType::Accept
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = window;
        true
    }
}

/// Show a modal address-bar dialog pre-filled with the current URL.
/// The user can edit the URL and press Enter or "Go" to navigate.
/// Returns the URL to navigate to, or None if the user cancelled.
/// Navigation is blocked (with an inline error) if the entered URL's domain
/// does not match the app domain or any entry in allowed_domains.
fn show_url_dialog(
    window: &tao::window::Window,
    url: &str,
    app_domain: &str,
    allowed_domains: &[String],
    excluded_domains: &[String],
) -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        use gtk::prelude::*;
        use tao::platform::unix::WindowExtUnix;
        let gtk_win = window.gtk_window();
        let parent: &gtk::Window = gtk_win.upcast_ref();
        let dialog = gtk::Dialog::with_buttons(
            Some("Navigate to URL"),
            Some(parent),
            gtk::DialogFlags::MODAL | gtk::DialogFlags::DESTROY_WITH_PARENT,
            &[
                ("Cancel", gtk::ResponseType::Cancel),
                ("Go", gtk::ResponseType::Accept),
            ],
        );
        let content = dialog.content_area();
        let entry = gtk::Entry::new();
        entry.set_text(url);
        entry.set_width_chars(60);
        let error_label = gtk::Label::new(None);
        content.pack_start(&entry, true, true, 8);
        content.pack_start(&error_label, false, false, 4);
        content.show_all();
        error_label.hide();
        entry.grab_focus();
        entry.select_region(0, -1);

        {
            let dialog_clone = dialog.clone();
            entry.connect_activate(move |_| {
                dialog_clone.response(gtk::ResponseType::Accept);
            });
        }

        let result = loop {
            match dialog.run() {
                gtk::ResponseType::Accept => {
                    let typed = entry.text().to_string();
                    if should_open_externally(&typed, app_domain, allowed_domains, excluded_domains) {
                        let allowed: Vec<&str> = std::iter::once(app_domain)
                            .chain(allowed_domains.iter().map(String::as_str))
                            .collect();
                        error_label.set_text(&format!(
                            "Domain not allowed. Permitted: {}",
                            allowed.join(", ")
                        ));
                        error_label.show();
                    } else {
                        break Some(typed);
                    }
                }
                _ => break None,
            }
        };
        dialog.close();
        result
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (window, url, app_domain, allowed_domains, excluded_domains);
        None
    }
}

fn popup_toast_script(msg: &str) -> String {
    format!(
        r#"(function(msg){{
  var t=document.createElement('div');
  t.style.cssText='position:fixed;top:20px;left:50%;transform:translateX(-50%);max-width:420px;word-break:break-all;background:rgba(37,99,235,0.92);color:#fff;padding:8px 12px;border-radius:6px;font-size:13px;font-family:sans-serif;z-index:2147483647;pointer-events:none;opacity:1;transition:opacity 0.4s ease';
  t.textContent=msg;
  document.body.appendChild(t);
  setTimeout(function(){{t.style.opacity='0';setTimeout(function(){{if(t.parentNode)t.parentNode.removeChild(t)}},400)}},3000);
}})('{}')"#,
        escape_js_string(msg)
    )
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
            &[],
            &[]
        ));
    }

    #[test]
    fn subdomain_of_app_stays_in_app() {
        assert!(!should_open_externally(
            "https://sub.mail.google.com/page",
            "mail.google.com",
            &[],
            &[]
        ));
    }

    #[test]
    fn different_domain_opens_externally() {
        assert!(should_open_externally(
            "https://example.com/link",
            "mail.google.com",
            &[],
            &[]
        ));
    }

    #[test]
    fn allowed_domain_stays_in_app() {
        let allowed = vec!["accounts.google.com".to_string()];
        assert!(!should_open_externally(
            "https://accounts.google.com/login",
            "mail.google.com",
            &allowed,
            &[]
        ));
    }

    #[test]
    fn allowed_parent_domain_matches_subdomains() {
        let allowed = vec!["google.com".to_string()];
        assert!(!should_open_externally(
            "https://accounts.google.com/login",
            "mail.google.com",
            &allowed,
            &[]
        ));
    }

    #[test]
    fn excluded_domain_opens_externally_even_if_allowed() {
        let allowed = vec!["google.com".to_string()];
        let excluded = vec!["meet.google.com".to_string()];
        assert!(should_open_externally(
            "https://meet.google.com/abc-def",
            "mail.google.com",
            &allowed,
            &excluded
        ));
    }

    #[test]
    fn excluded_parent_domain_matches_subdomains() {
        let excluded = vec!["google.com".to_string()];
        assert!(should_open_externally(
            "https://meet.google.com/abc-def",
            "mail.google.com",
            &[],
            &excluded
        ));
    }

    #[test]
    fn non_excluded_allowed_domain_stays_in_app() {
        let allowed = vec!["google.com".to_string()];
        let excluded = vec!["meet.google.com".to_string()];
        // calendar.google.com is allowed but not excluded, so stays in app
        assert!(!should_open_externally(
            "https://calendar.google.com/calendar",
            "mail.google.com",
            &allowed,
            &excluded
        ));
    }

    #[test]
    fn mailto_opens_externally() {
        assert!(should_open_externally(
            "mailto:user@example.com",
            "mail.google.com",
            &[],
            &[]
        ));
    }

    #[test]
    fn tel_opens_externally() {
        assert!(should_open_externally(
            "tel:+1234567890",
            "mail.google.com",
            &[],
            &[]
        ));
    }

    #[test]
    fn blob_stays_in_app() {
        assert!(!should_open_externally(
            "blob:https://mail.google.com/abc123",
            "mail.google.com",
            &[],
            &[]
        ));
    }

    #[test]
    fn about_blank_stays_in_app() {
        assert!(!should_open_externally(
            "about:blank",
            "mail.google.com",
            &[],
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

    // -- clipboard_image_paste_polyfill --

    #[test]
    fn clipboard_polyfill_contains_async_read() {
        let script = clipboard_image_paste_polyfill();
        assert!(script.contains("navigator.clipboard.read()"));
    }

    #[test]
    fn clipboard_polyfill_dispatches_paste_event() {
        let script = clipboard_image_paste_polyfill();
        assert!(script.contains("ClipboardEvent('paste'"));
    }

    #[test]
    fn clipboard_polyfill_guards_against_reentry() {
        let script = clipboard_image_paste_polyfill();
        assert!(script.contains("if (handling) return"));
    }

    #[test]
    fn clipboard_polyfill_skips_when_native_data_present() {
        let script = clipboard_image_paste_polyfill();
        assert!(script.contains("clipboardData.items.length > 0"));
    }

    #[test]
    fn clipboard_polyfill_dispatches_on_original_target() {
        let script = clipboard_image_paste_polyfill();
        assert!(script.contains("var target = e.target"));
        assert!(script.contains("target.dispatchEvent"));
    }

    #[test]
    fn clipboard_polyfill_filters_image_types_only() {
        let script = clipboard_image_paste_polyfill();
        assert!(script.contains("type.indexOf('image/') !== 0"));
    }

    #[test]
    fn clipboard_polyfill_handles_gettype_rejection() {
        let script = clipboard_image_paste_polyfill();
        assert!(script.contains(".catch(function() {"));
        assert!(script.contains("settle()"));
    }
}
