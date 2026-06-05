mod app;
mod commands;
mod db;
mod mail;
mod models;
mod oauth;
mod secret;
mod state;
mod tray;
mod window;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    app::run();
}
