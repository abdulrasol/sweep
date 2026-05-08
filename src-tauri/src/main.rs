// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// Learn more about Tauri commands at https://tauri.app/v1/guides/features/command
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[tauri::command]
fn scan_environment(path: &str) -> Vec<FileItem> {
    // In a real app, logic to scan the disk would go here
    vec![]
}

#[derive(serde::Serialize)]
struct FileItem {
    id: String,
    path: String,
    file_type: String,
    size: u64,
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![greet, scan_environment])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
