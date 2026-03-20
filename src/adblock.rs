use std::collections::HashSet;
use std::path::Path;

/// Default blocklist embedded at compile time (Peter Lowe's ad server list, CC BY-SA 4.0).
const DEFAULT_BLOCKLIST: &str = include_str!("../data/adblock-domains.txt");

fn parse_default_domains() -> Vec<&'static str> {
    DEFAULT_BLOCKLIST
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect()
}

fn load_custom_domains(custom_blocklist: Option<&Path>, config_dir: &Path) -> Vec<String> {
    let Some(path) = custom_blocklist else {
        return Vec::new();
    };
    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        config_dir.join(path)
    };
    let Ok(content) = std::fs::read_to_string(&resolved) else {
        return Vec::new();
    };
    content
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect()
}

/// Load blocked domains into a set for Rust-side URL checking (e.g. popup blocking).
pub fn load_blocked_domains(custom_blocklist: Option<&Path>, config_dir: &Path) -> HashSet<String> {
    let mut set: HashSet<String> = parse_default_domains().into_iter().map(|s| s.to_string()).collect();
    for d in load_custom_domains(custom_blocklist, config_dir) {
        set.insert(d);
    }
    set
}

/// Check if a URL's hostname matches a blocked domain (exact or parent domain match).
pub fn is_blocked(domains: &HashSet<String>, url: &str) -> bool {
    let Some(host) = extract_hostname(url) else {
        return false;
    };
    let mut h = host.as_str();
    loop {
        if domains.contains(h) {
            return true;
        }
        match h.find('.') {
            Some(i) => h = &h[i + 1..],
            None => return false,
        }
    }
}

/// Extract hostname from an HTTP/HTTPS URL without a URL-parsing dependency.
fn extract_hostname(url: &str) -> Option<String> {
    let after_scheme = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"))?;
    let host_port = match after_scheme.find('/') {
        Some(i) => &after_scheme[..i],
        None => after_scheme,
    };
    // Strip optional port
    let host = match host_port.rfind(':') {
        Some(i) => &host_port[..i],
        None => host_port,
    };
    if host.is_empty() {
        return None;
    }
    Some(host.to_ascii_lowercase())
}

/// Build the JavaScript that intercepts network APIs to block ad/tracker requests.
/// The script patches fetch, XHR, Image, sendBeacon, and uses a MutationObserver
/// to remove ad elements as they appear in the DOM.
pub fn build_script(custom_blocklist: Option<&Path>, config_dir: &Path) -> String {
    let mut domains = parse_default_domains();
    let custom = load_custom_domains(custom_blocklist, config_dir);
    for d in &custom {
        domains.push(d);
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

    #[test]
    fn is_blocked_matches_exact_domain() {
        let domains = load_blocked_domains(None, Path::new("."));
        assert!(is_blocked(&domains, "https://googlesyndication.com/some/path"));
        assert!(is_blocked(&domains, "https://doubleclick.net/"));
    }

    #[test]
    fn is_blocked_matches_subdomain() {
        let domains = load_blocked_domains(None, Path::new("."));
        assert!(is_blocked(
            &domains,
            "https://abc123.safeframe.googlesyndication.com/safeframe/1-0-45/html/container.html"
        ));
        assert!(is_blocked(
            &domains,
            "https://ep2.adtrafficquality.google/sodar/sodar2/253/runner.html"
        ));
    }

    #[test]
    fn is_blocked_allows_non_ad_domains() {
        let domains = load_blocked_domains(None, Path::new("."));
        assert!(!is_blocked(&domains, "https://www.google.com/"));
        assert!(!is_blocked(&domains, "https://example.com/page"));
    }

    #[test]
    fn extract_hostname_works() {
        assert_eq!(extract_hostname("https://example.com/path"), Some("example.com".into()));
        assert_eq!(extract_hostname("http://FOO.COM:8080/x"), Some("foo.com".into()));
        assert_eq!(extract_hostname("ftp://nope.com"), None);
        assert_eq!(extract_hostname("not-a-url"), None);
    }
}
