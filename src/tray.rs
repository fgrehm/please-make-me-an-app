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
