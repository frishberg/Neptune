// System tray icon and its menu. This is the only "window" most people will
// ever open. Everything here is one click and does one thing.

use std::time::Duration;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::AppHandle;

use crate::overlay;

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    // Three things. Everything else is in the window.
    let open = MenuItem::with_id(app, "open", "Open Neptune", true, None::<&str>)?;
    let hide_hour = MenuItem::with_id(app, "hide_hour", "Hide pill for 1 hour", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Neptune", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &hide_hour, &PredefinedMenuItem::separator(app)?, &quit])?;

    // Windows: the app icon at 32 px, drawn by scripts/make-icons.py with the
    // same proportions as the taskbar's icon, so the tray and taskbar match.
    // Mac: a black template image that the OS tints.
    #[cfg(target_os = "macos")]
    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray-mac-template@2x.png"))?;
    #[cfg(not(target_os = "macos"))]
    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray-win.png"))?;

    let tray = TrayIconBuilder::with_id("main")
        .icon(icon)
        .icon_as_template(cfg!(target_os = "macos"))
        .tooltip("Neptune — hold Ctrl+Space to dictate")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if let tauri::tray::TrayIconEvent::Click { button: tauri::tray::MouseButton::Left, button_state: tauri::tray::MouseButtonState::Up, .. } = event {
                overlay::show_main(tray.app_handle(), None);
            }
        })
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => overlay::show_main(app, None),
            "hide_hour" => hide_for(app, Duration::from_secs(3600)),
            "quit" => {
                // Take the icon down first: exiting with it still registered
                // leaves a ghost in the notification area until it is hovered.
                drop(app.remove_tray_by_id("main"));
                app.exit(0)
            }
            _ => {}
        })
        .build(app)?;
    let _ = tray;
    Ok(())
}

fn hide_for(app: &AppHandle, d: Duration) {
    overlay::hide(app);
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(d);
        overlay::show(&app);
    });
}
