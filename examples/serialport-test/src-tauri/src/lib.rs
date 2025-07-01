// Learn more about Tauri commands at https://tauri.app/v1/guides/features/command
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[tauri::command]
async fn get_ports_programmatically(
    app: tauri::AppHandle,
    serial: tauri::State<'_, tauri_plugin_serialplugin::desktop_api::SerialPort<tauri::Wry>>
) -> Result<String, String> {
    
    // Get list of available ports
    let available_ports = tauri_plugin_serialplugin::commands::available_ports(app.clone(), serial.clone())
        .map_err(|e| format!("Failed to get available ports: {}", e))?;
    
    // Get list of ports via direct commands
    let direct_ports = tauri_plugin_serialplugin::commands::available_ports_direct(app.clone(), serial.clone())
        .map_err(|e| format!("Failed to get direct ports: {}", e))?;
    
    // Get list of managed ports
    let managed_ports = tauri_plugin_serialplugin::commands::managed_ports(app.clone(), serial.clone())
        .map_err(|e| format!("Failed to get managed ports: {}", e))?;
    
    // Format the result
    let result = format!(
        "=== Programmatic Port List Retrieval ===\n\
        Available ports count: {}\n\
        Direct ports count: {}\n\
        Managed ports count: {}\n\
        \n\
        Available ports: {:?}\n\
        Direct ports: {:?}\n\
        Managed ports: {:?}\n\
        \n\
        === Programmatic retrieval completed ===",
        available_ports.len(),
        direct_ports.len(),
        managed_ports.len(),
        available_ports,
        direct_ports,
        managed_ports
    );
    
    Ok(result)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|_app| {
            #[cfg(debug_assertions)] // only include this code on debug builds
            {
                // Note: DevTools are not available on mobile
                use tauri::Manager;
                let window = _app.get_webview_window("main").unwrap();
                window.open_devtools();
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![greet, get_ports_programmatically])
        .plugin(tauri_plugin_serialplugin::init())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
