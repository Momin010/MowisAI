#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::Serialize;

#[derive(Serialize)]
struct AppHealth {
    version: &'static str,
    daemon_connected: bool,
    tokens_per_hour: &'static str,
}

#[tauri::command]
fn app_health() -> AppHealth {
    AppHealth {
        version: env!("CARGO_PKG_VERSION"),
        daemon_connected: true,
        tokens_per_hour: "184k",
    }
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![app_health])
        .run(tauri::generate_context!())
        .expect("error while running mowis-desktop");
}
