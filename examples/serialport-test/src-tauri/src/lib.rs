// Learn more about Tauri commands at https://tauri.app/v1/guides/features/command
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[cfg(desktop)]
#[tauri::command]
async fn get_ports_programmatically(
    app: tauri::AppHandle,
    serial: tauri::State<'_, tauri_plugin_serialplugin::desktop_api::SerialPort<tauri::Wry>>,
) -> Result<String, String> {
    let available_ports = tauri_plugin_serialplugin::commands::available_ports(app.clone(), serial.clone(), None)
        .map_err(|e| format!("Failed to get available ports: {}", e))?;

    let managed_ports = tauri_plugin_serialplugin::commands::managed_ports(app.clone(), serial.clone())
        .map_err(|e| format!("Failed to get managed ports: {}", e))?;

    Ok(format!(
        "=== Programmatic Port List Retrieval ===\n\
        Available ports count: {}\n\
        Managed ports count: {}\n\
        \n\
        Available ports: {:?}\n\
        Managed ports: {:?}\n\
        \n\
        === Programmatic retrieval completed ===",
        available_ports.len(),
        managed_ports.len(),
        available_ports,
        managed_ports
    ))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|_app| {
            #[cfg(all(debug_assertions, desktop))]
            {
                use tauri::Manager;
                let window = _app.get_webview_window("main").unwrap();
                window.open_devtools();
            }
            Ok(())
        })
        .invoke_handler({
            #[cfg(desktop)]
            {
                tauri::generate_handler![greet, get_ports_programmatically]
            }
            #[cfg(mobile)]
            {
                tauri::generate_handler![greet]
            }
        })
        .plugin(tauri_plugin_serialplugin::init())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
