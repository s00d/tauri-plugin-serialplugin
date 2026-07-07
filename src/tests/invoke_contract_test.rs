#[cfg(test)]
mod tests {
    use crate::api::desktop::SerialPort;
    use crate::commands::watch;
    use crate::events::{SerialEvent, WatchOptions};
    use serde::Deserialize;
    use std::sync::{Arc, Mutex};
    use tauri::ipc::Channel;
    use tauri::ipc::InvokeResponseBody;
    use tauri::test::MockRuntime;
    use tauri::Manager;

    #[derive(Deserialize)]
    struct WatchArgs {
        path: String,
        options: WatchOptions,
    }

    #[derive(Deserialize)]
    struct UnwatchArgs {
        #[serde(rename = "channelId")]
        channel_id: u32,
    }

    #[derive(Deserialize)]
    struct AvailablePortsArgs {
        #[serde(rename = "singlePortPerDevice", default)]
        single_port_per_device: bool,
    }

    #[test]
    fn watch_args_deserialize_from_js_payload() {
        let json = serde_json::json!({
            "path": "/dev/ttyUSB0",
            "options": {
                "timeout": 500,
                "size": 2048,
                "serialDataFlushIntervalMs": 250
            }
        });
        let args: WatchArgs = serde_json::from_value(json).unwrap();
        assert_eq!(args.path, "/dev/ttyUSB0");
        assert_eq!(args.options.timeout, Some(500));
        assert_eq!(args.options.size, Some(2048));
        assert_eq!(args.options.serial_data_flush_interval_ms, Some(250));
    }

    #[test]
    fn unwatch_args_deserialize_channel_id() {
        let json = serde_json::json!({ "channelId": 42 });
        let args: UnwatchArgs = serde_json::from_value(json).unwrap();
        assert_eq!(args.channel_id, 42);
    }

    #[test]
    fn available_ports_args_default_single_port_false() {
        let json = serde_json::json!({});
        let args: AvailablePortsArgs = serde_json::from_value(json).unwrap();
        assert!(!args.single_port_per_device);
    }

    #[test]
    fn available_ports_args_single_port_true() {
        let json = serde_json::json!({ "singlePortPerDevice": true });
        let args: AvailablePortsArgs = serde_json::from_value(json).unwrap();
        assert!(args.single_port_per_device);
    }

    #[test]
    fn watch_command_requires_open_port_on_desktop() {
        let app = tauri::test::mock_app();
        let serial_port = SerialPort::new(app.handle().clone());
        app.manage(serial_port);

        let events: Arc<Mutex<Vec<SerialEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let events_clone = events.clone();
        let channel = Channel::<SerialEvent>::new(move |body| {
            if let InvokeResponseBody::Json(json) = body {
                if let Ok(event) = serde_json::from_str::<SerialEvent>(&json) {
                    events_clone.lock().unwrap().push(event);
                }
            }
            Ok(())
        });

        let result = watch(
            app.handle().clone(),
            app.state::<SerialPort<MockRuntime>>(),
            "NONEXISTENT".to_string(),
            WatchOptions::default(),
            channel,
        );
        assert!(result.is_err());
    }

    #[test]
    fn exchange_options_deserialize_result_format_and_at_intermediate() {
        use crate::events::{ExchangeCompletionMode, ExchangeOptions};

        let json = serde_json::json!({
            "timeoutMs": 5000,
            "resultFormat": "numeric",
            "completionMode": "atIntermediate",
            "command": "AT+CMGS=1",
            "solicitedPrefixes": ["+CMGS:"]
        });
        let opts: ExchangeOptions = serde_json::from_value(json).unwrap();
        let resolved = opts.resolve();
        assert_eq!(
            resolved.result_format,
            crate::events::AtResultFormat::Numeric
        );
        assert_eq!(
            resolved.completion_mode,
            ExchangeCompletionMode::AtIntermediate
        );
        assert_eq!(resolved.command.as_deref(), Some("AT+CMGS=1"));
    }

    #[test]
    fn exchange_match_intermediate_serde_roundtrip() {
        use crate::at::parse::ExchangeMatch;

        let m = ExchangeMatch::Intermediate {
            line: ">".to_string(),
        };
        let json = serde_json::to_value(&m).unwrap();
        assert_eq!(json["intermediate"]["line"], ">");
        let back: ExchangeMatch = serde_json::from_value(json).unwrap();
        assert!(matches!(back, ExchangeMatch::Intermediate { line } if line == ">"));
    }

    #[test]
    fn exchange_match_numeric_ok_serde_roundtrip() {
        use crate::at::parse::ExchangeMatch;

        let m = ExchangeMatch::Ok;
        let json = serde_json::to_value(&m).unwrap();
        assert_eq!(json, "ok");
        let back: ExchangeMatch = serde_json::from_value(json).unwrap();
        assert!(matches!(back, ExchangeMatch::Ok));
    }
}
