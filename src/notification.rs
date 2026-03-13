use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Build the JavaScript that intercepts the Web Notification API and forwards
/// notifications to the host via IPC.
pub fn intercept_script() -> &'static str {
    r#"(function() {
    var OriginalNotification = window.Notification;
    function PmmaNotification(title, options) {
        options = options || {};
        window.ipc.postMessage(JSON.stringify({
            type: "notification",
            title: title,
            body: options.body || "",
            icon: options.icon || ""
        }));
        this.title = title;
        this.body = options.body || "";
        this.icon = options.icon || "";
        this.close = function() {};
        this.addEventListener = function() {};
        this.removeEventListener = function() {};
    }
    PmmaNotification.permission = "granted";
    PmmaNotification.requestPermission = function(callback) {
        if (callback) callback("granted");
        return Promise.resolve("granted");
    };
    PmmaNotification.maxActions = OriginalNotification ? OriginalNotification.maxActions : 0;
    window.Notification = PmmaNotification;
})();"#
}

/// Build JavaScript that intercepts alert/confirm/prompt dialogs.
///
/// WebKitGTK's native dialog implementation corrupts the webview's size
/// allocation inside a GTK container: after the dialog dismisses, the webview
/// renders at a smaller width, leaving part of the window empty. Replacing
/// these functions with IPC-based alternatives avoids the bug entirely.
///
/// - `alert()` forwards the message as a system notification via IPC.
/// - `confirm()` forwards the message and returns `true`.
/// - `prompt()` returns the default value (no dialog shown).
pub fn dialog_intercept_script() -> &'static str {
    r#"(function() {
    window.alert = function(msg) {
        window.ipc.postMessage(JSON.stringify({
            type: "notification",
            title: document.title || "Alert",
            body: String(msg || ""),
            icon: ""
        }));
    };
    window.confirm = function(msg) {
        window.ipc.postMessage(JSON.stringify({
            type: "notification",
            title: document.title || "Confirm",
            body: String(msg || ""),
            icon: ""
        }));
        return true;
    };
    window.prompt = function(msg, defaultValue) {
        return defaultValue !== undefined ? String(defaultValue) : null;
    };
})();"#
}

/// Handle an IPC message. If it is a notification message, show a system notification.
/// If `raise_flag` is provided, clicking the notification sets it to request the
/// event loop to raise the window.
pub fn handle_ipc(
    message: &str,
    app_name: &str,
    icon_path: Option<&Path>,
    raise_flag: Option<&Arc<AtomicBool>>,
) {
    let parsed: serde_json::Value = match serde_json::from_str(message) {
        Ok(v) => v,
        Err(_) => return,
    };

    if parsed.get("type").and_then(|t| t.as_str()) != Some("notification") {
        return;
    }

    let title = parsed
        .get("title")
        .and_then(|t| t.as_str())
        .unwrap_or(app_name);
    let body = parsed.get("body").and_then(|b| b.as_str()).unwrap_or("");

    show(title, body, app_name, icon_path, raise_flag);
}

fn show(
    title: &str,
    body: &str,
    app_name: &str,
    icon_path: Option<&Path>,
    raise_flag: Option<&Arc<AtomicBool>>,
) {
    let mut n = notify_rust::Notification::new();
    n.appname(app_name)
        .summary(title)
        .body(body)
        .action("default", "Open")
        .timeout(notify_rust::Timeout::Milliseconds(10_000));

    if let Some(path) = icon_path {
        n.icon(&path.display().to_string());
    }

    match n.show() {
        Ok(handle) => {
            if let Some(flag) = raise_flag {
                let flag = flag.clone();
                // Spawn a thread per notification to wait for a click action.
                // wait_for_action() blocks; it cannot be made async without
                // a different notify-rust API. The 10s timeout bounds thread
                // lifetime. process::exit(0) in the tray quit path ensures
                // these threads don't delay exit.
                std::thread::spawn(move || {
                    handle.wait_for_action(|action| {
                        if action == "default" {
                            flag.store(true, Ordering::Release);
                        }
                    });
                });
            }
        }
        Err(e) => eprintln!("Failed to show notification: {}", e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intercept_script_overrides_notification() {
        let script = intercept_script();
        assert!(script.contains("window.Notification = PmmaNotification"));
        assert!(script.contains("window.ipc.postMessage"));
        assert!(script.contains("requestPermission"));
    }

    #[test]
    fn handle_ipc_ignores_non_notification() {
        // Should not panic or show anything
        handle_ipc(r#"{"type": "other"}"#, "test", None, None);
    }

    #[test]
    fn handle_ipc_ignores_invalid_json() {
        handle_ipc("not json", "test", None, None);
    }

    #[test]
    fn intercept_script_grants_permission() {
        let script = intercept_script();
        assert!(script.contains(r#"permission = "granted""#));
        assert!(script.contains(r#"Promise.resolve("granted")"#));
    }

    #[test]
    fn dialog_intercept_overrides_alert() {
        let script = dialog_intercept_script();
        assert!(script.contains(r#"window.alert = function(msg)"#));
        assert!(script.contains(r#"title: document.title || "Alert""#));
    }

    #[test]
    fn dialog_intercept_overrides_confirm() {
        let script = dialog_intercept_script();
        assert!(script.contains(r#"window.confirm = function(msg)"#));
        assert!(script.contains(r#"title: document.title || "Confirm""#));
    }

    #[test]
    fn dialog_intercept_overrides_prompt() {
        let script = dialog_intercept_script();
        assert!(script.contains("window.prompt = function(msg, defaultValue)"));
    }
}
