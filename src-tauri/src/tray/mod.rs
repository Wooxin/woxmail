use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, TrayIconBuilder, TrayIconEvent},
};

pub fn init(app: &tauri::App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "打开", true, None::<&str>)?;
    let close = MenuItem::with_id(app, "close", "关闭", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;

    let menu = Menu::with_items(app, &[&open, &close, &quit])?;

    let _tray = TrayIconBuilder::new()
        .icon(app.default_window_icon().unwrap().clone())
        .menu(&menu)
        .tooltip("Wox Mail")
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => {
                crate::window::show_main(app);
            }
            "close" => {
                crate::window::hide_main(app);
            }
            "quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                ..
            } = event
            {
                crate::window::show_main(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}
