use anyhow::{Context, Result};
use tray_icon::menu::{Menu, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

pub struct TrayState {
    pub _icon: TrayIcon,
    pub quit_id: MenuId,
    pub toggle_id: MenuId,
}

pub fn create(tooltip: &str, icon_rgba: Option<(Vec<u8>, u32, u32)>) -> Result<TrayState> {
    let menu = Menu::new();
    let toggle_item = MenuItem::new("Show/Hide", true, None);
    let quit_item = MenuItem::new("Quit", true, None);

    let toggle_id = toggle_item.id().clone();
    let quit_id = quit_item.id().clone();

    menu.append(&toggle_item)
        .context("Failed to add Show/Hide menu item")?;
    menu.append(&PredefinedMenuItem::separator())
        .context("Failed to add menu separator")?;
    menu.append(&quit_item)
        .context("Failed to add Quit menu item")?;

    let icon = icon_from_rgba(icon_rgba);

    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip(tooltip)
        .with_icon(icon)
        .build()
        .context("Failed to create system tray icon")?;

    // On Linux, tray-icon uses the libappindicator/AppIndicator backend, and
    // its `with_tooltip` is a no-op (the GTK impl ignores the tooltip). tray-icon
    // also never sets the StatusNotifierItem `Title` (its `with_title` maps to
    // `app_indicator_set_label`, a different property). Tray hosts such as
    // waybar therefore fall back to the GTK app's `g_get_prgname()`, which we
    // set to `pmma-<app>` for WM_CLASS/icon matching -- so the hover text reads
    // `pmma-whatsapp` instead of the app's display title.
    //
    // The only way to set the real SNI Title is `app_indicator_set_title`, which
    // tray-icon does not expose. Reach the underlying AppIndicator via the
    // unsafe accessor and call `set_title` directly.
    #[cfg(target_os = "linux")]
    set_appindicator_title(&tray, tooltip);

    Ok(TrayState {
        _icon: tray,
        quit_id,
        toggle_id,
    })
}

fn icon_from_rgba(rgba: Option<(Vec<u8>, u32, u32)>) -> Icon {
    if let Some((data, width, height)) = rgba
        && let Ok(icon) = Icon::from_rgba(data, width, height) {
            return icon;
        }
    default_icon()
}

fn default_icon() -> Icon {
    // 16x16 blue square as fallback
    let size = 16u32;
    let pixel = [0x4A, 0x90, 0xD9, 0xFF];
    let rgba: Vec<u8> = pixel.repeat((size * size) as usize);
    // Hardcoded 16x16 RGBA buffer, always valid
    Icon::from_rgba(rgba, size, size).expect("failed to create default icon")
}

/// Set the AppIndicator/StatusNotifierItem `Title` so tray hosts (waybar, etc.)
/// show the app's display title on hover instead of the GTK `prgname`
/// (`pmma-<app>`).
///
/// tray-icon's appindicator backend does not expose this; see `create` for the
/// rationale. This is Linux-only and uses tray-icon's unsafe `app_indicator()`
/// accessor plus a const-cast to obtain `&mut` for the FFI setter.
#[cfg(target_os = "linux")]
fn set_appindicator_title(tray: &TrayIcon, title: &str) {
    use libappindicator::AppIndicator;
    // SAFETY: `app_indicator()` returns a pointer to the AppIndicator owned by
    // the TrayIcon. The pointer stays valid while the TrayIcon is alive (we only
    // borrow it for the duration of this call). `AppIndicator` is a thin Rust
    // wrapper around a raw GObject pointer (`air`); `set_title` performs a single
    // FFI call to `app_indicator_set_title` on that C object with no Rust-side
    // shared mutable state, so a const-to-mut cast here matches how these FFI
    // handles are conventionally used and introduces no real aliasing.
    let ptr = unsafe { tray.app_indicator() };
    if ptr.is_null() {
        return;
    }
    // SAFETY: see above. The pointer is non-null and validly aligned for
    // AppIndicator (it was returned from a live TrayIcon).
    let indicator: &mut AppIndicator = unsafe { &mut *(ptr as *mut AppIndicator) };
    indicator.set_title(title);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_icon_succeeds() {
        let icon = default_icon();
        // Just verify it doesn't panic
        let _ = icon;
    }

    #[test]
    fn icon_from_rgba_falls_back_on_none() {
        let icon = icon_from_rgba(None);
        let _ = icon;
    }
}
