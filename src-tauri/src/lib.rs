#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

/// Minimal Tauri boundary. Timer commands will call focus-return-core after the
/// Windows toolchain is available; the UI remains usable as a browser preview.
#[tauri::command]
fn app_name() -> &'static str {
    "Success Timer"
}

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![app_name])
        .run(tauri::generate_context!())
        .expect("error while running Success Timer");
}
