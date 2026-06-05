use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

pub fn open_or_focus(app: AppHandle, label: String, url: String) {
    if let Some(win) = app.get_webview_window(&label) {
        let _ = win.show();
        let _ = win.set_focus();
        return;
    }

    let _ = WebviewWindowBuilder::new(&app, label, WebviewUrl::External(url.parse().unwrap()))
        .title("Wox Mail")
        .inner_size(1400.0, 900.0)
        .resizable(true)
        .center()
        .build();
}

pub fn show_main(app: &AppHandle) {
    if let Some(win) = app
        .get_webview_window("main")
        .or_else(|| app.webview_windows().into_values().next())
    {
        let _ = win.show();
        let _ = win.set_focus();
    }
}

pub fn hide_main(app: &AppHandle) {
    if let Some(win) = app
        .get_webview_window("main")
        .or_else(|| app.webview_windows().into_values().next())
    {
        let _ = win.hide();
    }
}
