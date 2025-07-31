#[cfg(test)]
mod tests {
    use crate::desktop_api::SerialPort;
    use crate::state::{DataBits, FlowControl, Parity, StopBits};
    use tauri::test::MockRuntime;
    use tauri::Manager;
    use tauri::App;

    fn create_test_app() -> App<MockRuntime> {
        let app = tauri::test::mock_app();
        let serial_port = SerialPort::new(app.handle().clone());
        app.manage(serial_port);
        app
    }

    #[test]
    fn test_desktop_api_init() {
        let app = create_test_app();
        let serial_port = app.state::<SerialPort<MockRuntime>>();
        assert!(serial_port.available_ports().is_ok());
    }

    #[test]
    fn test_desktop_api_port_list() {
        let app = create_test_app();
        let serial_port = app.state::<SerialPort<MockRuntime>>();
        let ports = serial_port.available_ports().unwrap();
        assert!(!ports.is_empty(), "Expected non-empty ports list");
    }

    #[test]
    fn test_desktop_api_port_operations() {
        let app = create_test_app();
        let serial_port = app.state::<SerialPort<MockRuntime>>();
        
        // Test should expect error when opening non-existent port
        let result = serial_port.open(
            "NONEXISTENT".to_string(),
            9600,
            Some(DataBits::Eight),
            Some(FlowControl::None),
            Some(Parity::None),
            Some(StopBits::One),
            Some(1000),
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_desktop_api_port_settings() {
        let app = create_test_app();
        let serial_port = app.state::<SerialPort<MockRuntime>>();

        // Test should expect error when setting settings for non-existent port
        let result = serial_port.set_baud_rate(
            "NONEXISTENT".to_string(),
            115200,
        );
        assert!(result.is_err());

        // Check that port is not added to managed ports list
        let ports = serial_port.managed_ports().unwrap();
        assert!(!ports.contains(&"NONEXISTENT".to_string()));
    }

    #[test]
    fn test_desktop_api_error_handling() {
        let app = create_test_app();
        let serial_port = app.state::<SerialPort<MockRuntime>>();

        // Test working with non-existent port
        let result = serial_port.write(
            "NONEXISTENT".to_string(),
            "Test".to_string(),
        );
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("is not open") || err_msg.contains("No such file or directory") || err_msg.contains("not found"),
            "Expected error to contain 'is not open', 'No such file or directory' or 'not found', got: {}", err_msg);

        // Test reading from non-existent port
        let result = serial_port.read(
            "NONEXISTENT".to_string(),
            Some(1000),
            Some(1024),
        );
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("is not open") || err_msg.contains("No such file or directory") || err_msg.contains("not found"),
            "Expected error to contain 'is not open', 'No such file or directory' or 'not found', got: {}", err_msg);

        // Test closing non-existent port
        let result = serial_port.close("NONEXISTENT".to_string());
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("is not open") || err_msg.contains("No such file or directory") || err_msg.contains("not found"),
            "Expected error to contain 'is not open', 'No such file or directory' or 'not found', got: {}", err_msg);
    }

    #[test]
    fn test_desktop_api_control_signals() {
        let app = create_test_app();
        let serial_port = app.state::<SerialPort<MockRuntime>>();

        // Test should expect error when setting control signals for non-existent port
        let result = serial_port.write_request_to_send(
            "NONEXISTENT".to_string(),
            true,
        );
        assert!(result.is_err());

        let result = serial_port.read_clear_to_send(
            "NONEXISTENT".to_string(),
        );
        assert!(result.is_err());

        let result = serial_port.read_data_set_ready(
            "NONEXISTENT".to_string(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_available_ports() {
        let app = create_test_app();
        let serial_port = app.state::<SerialPort<MockRuntime>>();
        let result = serial_port.available_ports();
        assert!(result.is_ok());
        let ports = result.unwrap();
        // In CI/CD environment, there might be no USB devices connected
        // So we just check that the function returns successfully
        // The ports list can be empty in CI/CD
        println!("Available ports: {:?}", ports);
    }

    #[test]
    fn test_available_ports_direct() {
        let app = create_test_app();
        let serial_port = app.state::<SerialPort<MockRuntime>>();
        let result = serial_port.available_ports_direct();
        assert!(result.is_ok());
        let ports = result.unwrap();
        // In CI/CD environment, there might be no USB devices connected
        // So we just check that the function returns successfully
        // The ports list can be empty in CI/CD
        println!("Available ports (direct): {:?}", ports);
    }

    #[test]
    fn test_managed_ports() {
        let app = create_test_app();
        let serial_port = app.state::<SerialPort<MockRuntime>>();
        let result = serial_port.managed_ports();
        assert!(result.is_ok());
        let ports = result.unwrap();
        assert!(ports.is_empty());
    }

    #[test]
    fn test_open_port() {
        let app = create_test_app();
        let serial_port = app.state::<SerialPort<MockRuntime>>();

        // Test should expect error when opening non-existent port
        let result = serial_port.open(
            "NONEXISTENT".to_string(),
            9600,
            Some(DataBits::Eight),
            Some(FlowControl::None),
            Some(Parity::None),
            Some(StopBits::One),
            Some(1000),
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_close_port() {
        let app = create_test_app();
        let serial_port = app.state::<SerialPort<MockRuntime>>();

        // Test should expect error when closing non-existent port
        let result = serial_port.close("NONEXISTENT".to_string());
        assert!(result.is_err());
    }

    #[test]
    fn test_write_and_read() {
        let app = create_test_app();
        let serial_port = app.state::<SerialPort<MockRuntime>>();

        // Test should expect error when writing to non-existent port
        let result = serial_port.write(
            "NONEXISTENT".to_string(),
            "Test data".to_string(),
        );
        assert!(result.is_err());

        // Test should expect error when reading from non-existent port
        let result = serial_port.read(
            "NONEXISTENT".to_string(),
            Some(1000),
            Some(1024),
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_port_settings() {
        let app = create_test_app();
        let serial_port = app.state::<SerialPort<MockRuntime>>();

        // Test should expect error when setting settings for non-existent port
        let result = serial_port.set_baud_rate(
            "NONEXISTENT".to_string(),
            115200,
        );
        assert!(result.is_err());

        let result = serial_port.set_data_bits(
            "NONEXISTENT".to_string(),
            DataBits::Seven,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_control_signals() {
        let app = create_test_app();
        let serial_port = app.state::<SerialPort<MockRuntime>>();

        // Test should expect error when setting control signals for non-existent port
        let result = serial_port.write_request_to_send(
            "NONEXISTENT".to_string(),
            true,
        );
        assert!(result.is_err());

        let result = serial_port.write_data_terminal_ready(
            "NONEXISTENT".to_string(),
            true,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_buffer_operations() {
        let app = create_test_app();
        let serial_port = app.state::<SerialPort<MockRuntime>>();

        // Test should expect error when performing buffer operations on non-existent port
        let result = serial_port.clear_buffer(
            "NONEXISTENT".to_string(),
            crate::state::ClearBuffer::All,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_break_control() {
        let app = create_test_app();
        let serial_port = app.state::<SerialPort<MockRuntime>>();

        // Test should expect error when setting break on non-existent port
        let result = serial_port.set_break("NONEXISTENT".to_string());
        assert!(result.is_err());

        // Test should expect error when clearing break on non-existent port
        let result = serial_port.clear_break("NONEXISTENT".to_string());
        assert!(result.is_err());
    }
} 
