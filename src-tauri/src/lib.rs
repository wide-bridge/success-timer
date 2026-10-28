#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

/// Tauri boundary for the single-window Success Timer desktop app.
#[tauri::command]
fn app_name() -> &'static str {
    "Success Timer"
}

pub fn run() {
    #[cfg(windows)]
    if !single_instance() {
        return;
    }

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![app_name])
        .run(tauri::generate_context!())
        .expect("error while running Success Timer");
}

#[cfg(windows)]
fn single_instance() -> bool {
    use std::ptr::null_mut;
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS};
    use windows_sys::Win32::System::Threading::CreateMutexW;

    let name: Vec<u16> = "Global\\SuccessTimerSingleInstance\0".encode_utf16().collect();
    let handle = unsafe { CreateMutexW(null_mut(), 0, name.as_ptr()) };
    if handle.is_null() {
        return true;
    }
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        unsafe { CloseHandle(handle); }
        return false;
    }
    // Keep the mutex alive for the lifetime of this process. Windows releases
    // it automatically when the process exits.
    let _keep_handle = handle;
    true
}
