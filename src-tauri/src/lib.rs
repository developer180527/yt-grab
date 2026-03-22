mod commands;

use std::collections::HashMap;
use std::process::Child;
use std::sync::{Arc, Mutex};
use rusqlite::Connection;
use tauri::Manager;  // ← add this

pub use commands::AppState;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let app_data = app.path().app_data_dir()
                .expect("could not resolve app data dir");
            std::fs::create_dir_all(&app_data)?;
            let db_path = app_data.join("history.db");
            let conn = Connection::open(db_path)
                .expect("failed to open SQLite database");

            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS history (
                    id          INTEGER PRIMARY KEY AUTOINCREMENT,
                    url         TEXT NOT NULL,
                    title       TEXT NOT NULL,
                    thumbnail   TEXT,
                    format_id   TEXT NOT NULL,
                    audio_only  INTEGER NOT NULL DEFAULT 0,
                    output_path TEXT,
                    status      TEXT NOT NULL,
                    created_at  TEXT NOT NULL
                );"
            ).expect("failed to create history table");

            app.manage(AppState {
                downloads: Arc::new(Mutex::new(HashMap::<String, Child>::new())),
                db: Arc::new(Mutex::new(conn)),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::check_ytdlp,
            commands::fetch_media_info,
            commands::start_download,
            commands::cancel_download,
            commands::open_path,
            commands::get_history,
            commands::delete_history_item,
            commands::clear_history,
        ])
        .run(tauri::generate_context!())
        .expect("error while running yt-grab");
}
