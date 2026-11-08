//! Commands through the real Tauri IPC path (argument names, serialization,
//! async command execution), not by calling the Rust functions directly.
//! Registered as app commands: `mock_context` has no plugin ACL manifests, and the
//! ACL wiring itself is covered by `command_inventory_test` / `acl_smoke_test`.

#[cfg(test)]
mod tests {
    use tauri::ipc::{CallbackFn, InvokeBody};
    use tauri::test::{get_ipc_response, mock_builder, mock_context, noop_assets, INVOKE_KEY};
    use tauri::webview::InvokeRequest;
    use tauri::Manager;

    fn webview() -> (
        tauri::App<tauri::test::MockRuntime>,
        tauri::WebviewWindow<tauri::test::MockRuntime>,
    ) {
        let app = mock_builder()
            .invoke_handler(tauri::generate_handler![
                crate::commands::managed_ports,
                crate::commands::open,
            ])
            .build(mock_context(noop_assets()))
            .expect("build mock app");
        app.manage(crate::api::SerialPort::new(app.handle().clone()));
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("build webview");
        (app, webview)
    }

    fn request(cmd: &str, body: serde_json::Value) -> InvokeRequest {
        InvokeRequest {
            cmd: cmd.into(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            // The app's own (local) origin; tauri rejects commands from others.
            url: if cfg!(windows) {
                "http://tauri.localhost"
            } else {
                "tauri://localhost"
            }
            .parse()
            .expect("valid invoke URL"),
            body: InvokeBody::Json(body),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_string(),
        }
    }

    #[test]
    fn managed_ports_over_ipc_is_empty() {
        let (_app, webview) = webview();
        let ports = get_ipc_response(&webview, request("managed_ports", serde_json::json!({})))
            .expect("managed_ports")
            .deserialize::<Vec<String>>()
            .expect("Vec<String>");
        assert!(ports.is_empty());
    }

    /// Async command with camelCase args: a missing device is an error, not a hang.
    #[test]
    fn open_missing_port_over_ipc_returns_error() {
        let (_app, webview) = webview();
        let err = get_ipc_response(
            &webview,
            request(
                "open",
                serde_json::json!({ "path": "/dev/does-not-exist", "baudRate": 9600 }),
            ),
        )
        .expect_err("open of a missing port must fail");
        assert!(!err.to_string().is_empty());
    }
}
