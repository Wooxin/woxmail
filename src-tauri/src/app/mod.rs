use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let db = crate::db::Db::new(app);
            app.manage(crate::state::AppState { db });
            crate::tray::init(app)?;
            if let Some(win) = app.webview_windows().into_values().next() {
                let _ = win.show();
                let _ = win.set_focus();
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            crate::commands::add_message_tag,
            crate::commands::clear_message_tags,
            crate::commands::create_account,
            crate::commands::delete_account,
            crate::commands::gmail_oauth_login,
            crate::commands::get_account_settings,
            crate::commands::get_message,
            crate::commands::list_accounts,
            crate::commands::list_folders,
            crate::commands::list_messages,
            crate::commands::list_unread_counts,
            crate::commands::mark_message_read,
            crate::commands::move_messages_to_folder,
            crate::commands::open_mail,
            crate::commands::outlook_oauth_login,
            crate::commands::send_message,
            crate::commands::set_account_settings,
            crate::commands::sync_folder,
            crate::commands::sync_inbox
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri");
}
