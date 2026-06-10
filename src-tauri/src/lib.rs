mod app;
mod commands;
mod contacts;
mod db;
mod imports;
mod mail;
mod models;
mod oauth;
mod secret;
mod state;
mod text;
mod translate;
mod tray;
mod window;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    app::run();
}
