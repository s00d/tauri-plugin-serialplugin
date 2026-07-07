#[cfg(test)]
mod tests {
    use crate::api::desktop::SerialPort;
    use crate::commands::available_ports;
    use crate::port::list::apply_single_port_per_device;
    use std::collections::HashMap;
    use tauri::test::MockRuntime;
    use tauri::Manager;

    fn port_info(type_: &str) -> HashMap<String, String> {
        HashMap::from([("type".to_string(), type_.to_string())])
    }

    #[test]
    fn command_layer_passes_single_port_option() {
        let app = tauri::test::mock_app();
        let serial_port = SerialPort::new(app.handle().clone());
        app.manage(serial_port);

        let result = available_ports(
            app.handle().clone(),
            app.state::<SerialPort<MockRuntime>>(),
            Some(true),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn port_list_dedup_integrated_with_filter() {
        let ports = HashMap::from([
            ("/dev/tty.usbmodem1".to_string(), port_info("USB")),
            ("/dev/cu.usbmodem1".to_string(), port_info("USB")),
        ]);
        let filtered = apply_single_port_per_device(ports, true);
        assert_eq!(filtered.len(), 1);
        assert!(filtered.contains_key("/dev/cu.usbmodem1"));
    }
}
