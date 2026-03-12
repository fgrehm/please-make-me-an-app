use std::path::Path;

/// Default blocklist embedded at compile time (Peter Lowe's ad server list, CC BY-SA 4.0).
const DEFAULT_BLOCKLIST: &str = include_str!("../data/adblock-domains.txt");

/// Build the JavaScript that intercepts network APIs to block ad/tracker requests.
/// The script patches fetch, XHR, Image, sendBeacon, and uses a MutationObserver
/// to remove ad elements as they appear in the DOM.
pub fn build_script(custom_blocklist: Option<&Path>, config_dir: &Path) -> String {
    let mut domains: Vec<&str> = DEFAULT_BLOCKLIST
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();

    let custom_content;
    if let Some(path) = custom_blocklist {
        let resolved = if path.is_absolute() {
            path.to_path_buf()
        } else {
            config_dir.join(path)
        };
        if let Ok(content) = std::fs::read_to_string(&resolved) {
            custom_content = content;
            for line in custom_content.lines() {
                let line = line.trim();
                if !line.is_empty() && !line.starts_with('#') {
                    domains.push(line);
                }
            }
        }
    }

    let domain_list = domains
        .iter()
        .map(|d| format!("\"{}\"", d))
        .collect::<Vec<_>>()
        .join(",");

    format!(
        r#"(function() {{
    var BLOCKED = new Set([{domains}]);

    function isBlocked(url) {{
        try {{
            var h = new URL(url, location.href).hostname;
            while (h) {{
                if (BLOCKED.has(h)) return true;
                var i = h.indexOf('.');
                if (i < 0) break;
                h = h.substring(i + 1);
            }}
        }} catch(e) {{}}
        return false;
    }}

    // --- fetch ---
    var _fetch = window.fetch;
    window.fetch = function(input, init) {{
        var url = typeof input === 'string' ? input : (input && input.url) || '';
        if (isBlocked(url)) return Promise.reject(new TypeError('blocked'));
        return _fetch.apply(this, arguments);
    }};

    // --- XMLHttpRequest ---
    var _xhrOpen = XMLHttpRequest.prototype.open;
    XMLHttpRequest.prototype.open = function(method, url) {{
        this._pmmaBlocked = isBlocked(url);
        return _xhrOpen.apply(this, arguments);
    }};
    var _xhrSend = XMLHttpRequest.prototype.send;
    XMLHttpRequest.prototype.send = function() {{
        if (this._pmmaBlocked) {{
            Object.defineProperty(this, 'status', {{value: 0}});
            Object.defineProperty(this, 'readyState', {{value: 4}});
            this.dispatchEvent(new Event('error'));
            return;
        }}
        return _xhrSend.apply(this, arguments);
    }};

    // --- Image ---
    var _imgSrcSet = Object.getOwnPropertyDescriptor(HTMLImageElement.prototype, 'src').set;
    var _imgSrcGet = Object.getOwnPropertyDescriptor(HTMLImageElement.prototype, 'src').get;
    Object.defineProperty(HTMLImageElement.prototype, 'src', {{
        set: function(url) {{
            if (isBlocked(url)) return;
            _imgSrcSet.call(this, url);
        }},
        get: function() {{ return _imgSrcGet.call(this); }}
    }});

    // --- sendBeacon ---
    if (navigator.sendBeacon) {{
        var _beacon = navigator.sendBeacon.bind(navigator);
        navigator.sendBeacon = function(url, data) {{
            if (isBlocked(url)) return true;
            return _beacon(url, data);
        }};
    }}

    // --- MutationObserver: remove ad scripts/iframes ---
    new MutationObserver(function(mutations) {{
        for (var i = 0; i < mutations.length; i++) {{
            var nodes = mutations[i].addedNodes;
            for (var j = 0; j < nodes.length; j++) {{
                var n = nodes[j];
                if (n.nodeType !== 1) continue;
                var src = n.src || n.href || '';
                if (src && isBlocked(src)) {{ n.remove(); continue; }}
                if (n.tagName === 'IFRAME' && n.src && isBlocked(n.src)) n.remove();
            }}
        }}
    }}).observe(document.documentElement, {{childList: true, subtree: true}});
}})();"#,
        domains = domain_list
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn default_blocklist_parses() {
        let domains: Vec<&str> = DEFAULT_BLOCKLIST
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect();
        assert!(domains.len() > 1000, "expected 1000+ domains, got {}", domains.len());
    }

    #[test]
    fn build_script_contains_blocked_domains() {
        let script = build_script(None, Path::new("."));
        assert!(script.contains("doubleclick.net"));
        assert!(script.contains("googlesyndication.com"));
    }

    #[test]
    fn build_script_patches_fetch() {
        let script = build_script(None, Path::new("."));
        assert!(script.contains("window.fetch"));
        assert!(script.contains("XMLHttpRequest"));
        assert!(script.contains("sendBeacon"));
        assert!(script.contains("MutationObserver"));
    }

    #[test]
    fn build_script_with_custom_list() {
        let dir = std::env::temp_dir().join("pmma-test-adblock");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("custom.txt"), "# comment\ncustom-ad.example.com\n").unwrap();

        let script = build_script(Some(Path::new("custom.txt")), &dir);
        assert!(script.contains("custom-ad.example.com"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn build_script_ignores_missing_custom_list() {
        let script = build_script(Some(Path::new("nonexistent.txt")), Path::new("/tmp"));
        // Should still produce a valid script with default domains
        assert!(script.contains("doubleclick.net"));
    }
}
