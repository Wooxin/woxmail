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
            crate::commands::apply_filters,
            crate::commands::cancel_outbox_job,
            crate::commands::clear_cache,
            crate::commands::clear_message_tags,
            crate::commands::create_account,
            crate::commands::create_contact,
            crate::commands::create_filter_rule,
            crate::commands::delete_account,
            crate::commands::delete_contact,
            crate::commands::delete_filter_rule,
            crate::commands::delete_compose_draft,
            crate::commands::get_cache_settings,
            crate::commands::get_cache_stats,
            crate::commands::gmail_oauth_login,
            crate::commands::get_account_settings,
            crate::commands::import_contacts,
            crate::commands::import_eml,
            crate::commands::import_mbox,
            crate::commands::get_compose_draft,
            crate::commands::get_message,
            crate::commands::list_accounts,
            crate::commands::list_contacts,
            crate::commands::list_filter_rules,
            crate::commands::list_folders,
            crate::commands::list_messages,
            crate::commands::list_outbox_jobs,
            crate::commands::list_unread_counts,
            crate::commands::mark_message_read,
            crate::commands::move_messages_to_folder,
            crate::commands::open_attachment,
            crate::commands::open_mail,
            crate::commands::outlook_oauth_login,
            crate::commands::process_outbox,
            crate::commands::purge_old_messages,
            crate::commands::retry_outbox_job,
            crate::commands::save_attachment,
            crate::commands::save_compose_draft,
            crate::commands::search_messages,
            crate::commands::send_message,
            crate::commands::set_account_settings,
            crate::commands::set_cache_settings,
            crate::commands::sync_folder,
            crate::commands::sync_folder_deep,
            crate::commands::sync_inboxes,
            crate::commands::sync_inbox,
            crate::commands::toggle_filter_rule,
            crate::commands::translate_message,
            crate::commands::update_contact
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri");
}
