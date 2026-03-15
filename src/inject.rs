use crate::config::AppConfig;
use anyhow::{Context, Result};
use std::path::Path;

pub fn build_init_script(config: &AppConfig, config_dir: &Path) -> Result<Option<String>> {
    let mut parts = Vec::new();

    let css = resolve_content(
        config.inject.css.as_deref(),
        config.inject.css_file.as_deref(),
        config_dir,
    )?;
    if let Some(css) = css {
        let escaped = css.replace('\\', "\\\\").replace('`', "\\`");
        parts.push(format!(
            r#"(function() {{
    const style = document.createElement('style');
    style.textContent = `{escaped}`;
    document.head.appendChild(style);
}})();"#
        ));
    }

    let js = resolve_content(
        config.inject.js.as_deref(),
        config.inject.js_file.as_deref(),
        config_dir,
    )?;
    if let Some(js) = js {
        parts.push(js);
    }

    if parts.is_empty() {
        Ok(None)
    } else {
        Ok(Some(parts.join("\n\n")))
    }
}

/// Load inject content from inline string, file reference, or both.
/// If both are provided, file content comes first, then inline content.
pub(crate) fn resolve_content(
    inline: Option<&str>,
    file: Option<&Path>,
    config_dir: &Path,
) -> Result<Option<String>> {
    let mut content = String::new();

    if let Some(file_path) = file {
        let resolved = if file_path.is_absolute() {
            file_path.to_path_buf()
        } else {
            config_dir.join(file_path)
        };
        let file_content = std::fs::read_to_string(&resolved)
            .with_context(|| format!("Failed to read inject file: {}", resolved.display()))?;
        content.push_str(&file_content);
    }

    if let Some(inline_content) = inline {
        if !content.is_empty() {
            content.push('\n');
        }
        content.push_str(inline_content);
    }

    if content.is_empty() {
        Ok(None)
    } else {
        Ok(Some(content))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::InjectConfig;
    use std::io::Write;

    fn config_with_inject(inject: InjectConfig) -> crate::config::AppConfig {
        let mut config = crate::config::test_config();
        config.inject = inject;
        config
    }

    #[test]
    fn no_inject_returns_none() {
        let config = config_with_inject(InjectConfig::default());
        let result = build_init_script(&config, Path::new(".")).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn inline_css_wraps_in_style_element() {
        let config = config_with_inject(InjectConfig {
            css: Some("body { color: red; }".to_string()),
            ..InjectConfig::default()
        });
        let script = build_init_script(&config, Path::new(".")).unwrap().unwrap();
        assert!(script.contains("createElement('style')"));
        assert!(script.contains("body { color: red; }"));
    }

    #[test]
    fn inline_js_passed_through() {
        let config = config_with_inject(InjectConfig {
            js: Some("console.log('hello');".to_string()),
            ..InjectConfig::default()
        });
        let script = build_init_script(&config, Path::new(".")).unwrap().unwrap();
        assert!(script.contains("console.log('hello');"));
        assert!(!script.contains("createElement"));
    }

    #[test]
    fn css_file_loaded_and_wrapped() {
        let dir = std::env::temp_dir().join("pmma-test-inject-css");
        std::fs::create_dir_all(&dir).unwrap();
        let css_path = dir.join("test.css");
        let mut f = std::fs::File::create(&css_path).unwrap();
        write!(f, "h1 {{ font-size: 2em; }}").unwrap();

        let config = config_with_inject(InjectConfig {
            css_file: Some("test.css".into()),
            ..InjectConfig::default()
        });
        let script = build_init_script(&config, &dir).unwrap().unwrap();
        assert!(script.contains("h1 { font-size: 2em; }"));
        assert!(script.contains("createElement('style')"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn js_file_loaded() {
        let dir = std::env::temp_dir().join("pmma-test-inject-js");
        std::fs::create_dir_all(&dir).unwrap();
        let js_path = dir.join("test.js");
        std::fs::write(&js_path, "alert('hi');").unwrap();

        let config = config_with_inject(InjectConfig {
            js_file: Some("test.js".into()),
            ..InjectConfig::default()
        });
        let script = build_init_script(&config, &dir).unwrap().unwrap();
        assert!(script.contains("alert('hi');"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn file_and_inline_combined() {
        let dir = std::env::temp_dir().join("pmma-test-inject-combined");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("base.js"), "var x = 1;").unwrap();

        let config = config_with_inject(InjectConfig {
            js: Some("var y = 2;".to_string()),
            js_file: Some("base.js".into()),
            ..InjectConfig::default()
        });
        let script = build_init_script(&config, &dir).unwrap().unwrap();
        assert!(script.contains("var x = 1;"));
        assert!(script.contains("var y = 2;"));
        // File content comes before inline
        let x_pos = script.find("var x = 1;").unwrap();
        let y_pos = script.find("var y = 2;").unwrap();
        assert!(x_pos < y_pos, "file content should come before inline");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_file_returns_error() {
        let config = config_with_inject(InjectConfig {
            css_file: Some("nonexistent.css".into()),
            ..InjectConfig::default()
        });
        let result = build_init_script(&config, Path::new("/tmp"));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("nonexistent.css"));
    }

    #[test]
    fn absolute_file_path_not_joined_with_config_dir() {
        let dir = std::env::temp_dir().join("pmma-test-inject-abs");
        std::fs::create_dir_all(&dir).unwrap();
        let abs_path = dir.join("abs.css");
        std::fs::write(&abs_path, "p { margin: 0; }").unwrap();

        let config = config_with_inject(InjectConfig {
            css_file: Some(abs_path),
            ..InjectConfig::default()
        });
        // config_dir is irrelevant for absolute paths
        let script = build_init_script(&config, Path::new("/nonexistent"))
            .unwrap()
            .unwrap();
        assert!(script.contains("p { margin: 0; }"));

        std::fs::remove_dir_all(&dir).ok();
    }
}
