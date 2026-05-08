// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod cleanup_engine;
use cleanup_engine::CleanupItem;
use tauri_plugin_dialog::DialogExt;

#[tauri::command]
async fn scan_environment(path: String, modules: Vec<String>, ignored_paths: Vec<String>) -> Result<Vec<CleanupItem>, String> {
    if path.is_empty() {
        return Err("No path provided".to_string());
    }
    Ok(cleanup_engine::scan_directory(&path, modules, ignored_paths))
}

#[tauri::command]
async fn cleanup_item(item: CleanupItem) -> Result<(), String> {
    cleanup_engine::delete_item(&item.path)
}

#[tauri::command]
fn get_app_version(app: tauri::AppHandle) -> String {
    app.package_info().version.to_string()
}

#[tauri::command]
async fn select_directory(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    
    app.dialog().file().pick_folder(move |folder| {
        let path = folder.map(|f| f.to_string());
        tx.send(path).unwrap();
    });

    Ok(rx.recv().unwrap_or(None))
}

#[derive(serde::Serialize)]
struct SystemInfo {
    os_name: String,
    os_version: String,
    cpu_usage: f32,
    ram_total: u64,
    ram_used: u64,
    disk_total: u64,
    disk_free: u64,
}

#[tauri::command]
fn get_system_info() -> SystemInfo {
    use sysinfo::{System, Disks};
    let mut sys = System::new_all();
    sys.refresh_all();
    
    // For CPU usage to be accurate, we'd ideally wait, but even a single refresh 
    // provides the current state relative to the last refresh (or zero on first call).
    // In a polling scenario, subsequent calls will be accurate.
    let cpu_usage = sys.global_cpu_info().cpu_usage();
    
    let ram_total = sys.total_memory();
    let ram_used = sys.used_memory();
    
    let disks = Disks::new_with_refreshed_list();
    let mut disk_total = 0;
    let mut disk_free = 0;
    
    for disk in &disks {
        disk_total += disk.total_space();
        disk_free += disk.available_space();
    }

    SystemInfo {
        os_name: System::name().unwrap_or_else(|| "Unknown".to_string()),
        os_version: System::os_version().unwrap_or_else(|| "Unknown".to_string()),
        cpu_usage,
        ram_total,
        ram_used,
        disk_total,
        disk_free,
    }
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
        .invoke_handler(tauri::generate_handler![
            scan_environment,
            cleanup_item,
            select_directory,
            get_system_info,
            get_app_version
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
