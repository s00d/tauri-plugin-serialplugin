#[cfg(test)]
#[cfg(mobile)]
mod tests {
    use crate::mobile_api::SerialPort;
    use crate::state::{DataBits, FlowControl, Parity, StopBits};
    use crate::error::Error;
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::Duration;
    use tauri::{App, Manager, Runtime, State};
    use tauri::test::MockRuntime;
    use crate::tests::mock::{MockSerialPort, create_mock_serialport_info};

    fn create_test_serial_port() -> SerialPort<MockRuntime> {
        let app = tauri::test::mock_app();
        SerialPort::new(app.handle().clone())
    }

    #[test]
    fn test_mobile_api_init() {
        let app = tauri::test::mock_app();
        let serial_port = SerialPort::new(app.handle().clone());
        app.manage(serial_port);

        // Check that port is initialized
        let ports = app.state::<SerialPort<MockRuntime>>().managed_ports().unwrap();
        assert!(ports.is_empty());
    }

    #[test]
    fn test_open_port() {
        let app = tauri::test::mock_app();
        let serial_port = SerialPort::new(app.handle().clone());
        app.manage(serial_port);

        let result = app.state::<SerialPort<MockRuntime>>().open(
            "COM1".to_string(),
            9600,
            Some(DataBits::Eight),
            Some(FlowControl::None),
            Some(Parity::None),
            Some(StopBits::One),
            Some(1000),
        );
        assert!(result.is_ok());

        let ports = app.state::<SerialPort<MockRuntime>>().managed_ports().unwrap();
        assert!(ports.contains(&"COM1".to_string()));
    }

    #[test]
    fn test_write_and_read() {
        let app = tauri::test::mock_app();
        let serial_port = SerialPort::new(app.handle().clone());
        app.manage(serial_port);

        // Open port
        app.state::<SerialPort<MockRuntime>>().open(
            "COM1".to_string(),
            9600,
            Some(DataBits::Eight),
            Some(FlowControl::None),
            Some(Parity::None),
            Some(StopBits::One),
            Some(1000),
        ).unwrap();

        // Write data
        let write_result = app.state::<SerialPort<MockRuntime>>().write(
            "COM1".to_string(),
            "Test data".to_string(),
        );
        assert!(write_result.is_ok());
        assert_eq!(write_result.unwrap(), 9);

        // Read data
        let read_result = app.state::<SerialPort<MockRuntime>>().read(
            "COM1".to_string(),
            Some(1000),
            Some(1024),
        );
        assert!(read_result.is_ok());
        assert_eq!(read_result.unwrap(), "Test data");
    }

    #[test]
    fn test_port_settings() {
        let app = tauri::test::mock_app();
        let serial_port = SerialPort::new(app.handle().clone());
        app.manage(serial_port);

        // Open port
        app.state::<SerialPort<MockRuntime>>().open(
            "COM1".to_string(),
            9600,
            Some(DataBits::Eight),
            Some(FlowControl::None),
            Some(Parity::None),
            Some(StopBits::One),
            Some(1000),
        ).unwrap();

        // Set baud rate
        let result = app.state::<SerialPort<MockRuntime>>().set_baud_rate(
            "COM1".to_string(),
            115200,
        );
        assert!(result.is_ok());

        // Set data bits
        let result = app.state::<SerialPort<MockRuntime>>().set_data_bits(
            "COM1".to_string(),
            DataBits::Seven,
        );
        assert!(result.is_ok());

        // Set flow control
        let result = app.state::<SerialPort<MockRuntime>>().set_flow_control(
            "COM1".to_string(),
            FlowControl::Hardware,
        );
        assert!(result.is_ok());

        // Set parity
        let result = app.state::<SerialPort<MockRuntime>>().set_parity(
            "COM1".to_string(),
            Parity::Even,
        );
        assert!(result.is_ok());

        // Set stop bits
        let result = app.state::<SerialPort<MockRuntime>>().set_stop_bits(
            "COM1".to_string(),
            StopBits::Two,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_close_port() {
        let app = tauri::test::mock_app();
        let serial_port = SerialPort::new(app.handle().clone());
        app.manage(serial_port);

        // Open port
        app.state::<SerialPort<MockRuntime>>().open(
            "COM1".to_string(),
            9600,
            Some(DataBits::Eight),
            Some(FlowControl::None),
            Some(Parity::None),
            Some(StopBits::One),
            Some(1000),
        ).unwrap();

        // Close port
        let result = app.state::<SerialPort<MockRuntime>>().close("COM1".to_string());
        assert!(result.is_ok());

        // Check that port is closed
        let ports = app.state::<SerialPort<MockRuntime>>().managed_ports().unwrap();
        assert!(!ports.contains(&"COM1".to_string()));
    }

    #[test]
    fn test_control_signals() {
        let app = tauri::test::mock_app();
        let serial_port = SerialPort::new(app.handle().clone());
        app.manage(serial_port);

        // Open port
        app.state::<SerialPort<MockRuntime>>().open(
            "COM1".to_string(),
            9600,
            Some(DataBits::Eight),
            Some(FlowControl::None),
            Some(Parity::None),
            Some(StopBits::One),
            Some(1000),
        ).unwrap();

        // Test RTS
        let result = app.state::<SerialPort<MockRuntime>>().write_request_to_send(
            "COM1".to_string(),
            true,
        );
        assert!(result.is_ok());

        let result = app.state::<SerialPort<MockRuntime>>().read_clear_to_send(
            "COM1".to_string(),
        );
        assert!(result.is_ok());
        assert!(result.unwrap());

        // Test DTR
        let result = app.state::<SerialPort<MockRuntime>>().write_data_terminal_ready(
            "COM1".to_string(),
            true,
        );
        assert!(result.is_ok());

        let result = app.state::<SerialPort<MockRuntime>>().read_data_set_ready(
            "COM1".to_string(),
        );
        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[test]
    fn test_buffer_operations() {
        let app = tauri::test::mock_app();
        let serial_port = SerialPort::new(app.handle().clone());
        app.manage(serial_port);

        // Open port
        app.state::<SerialPort<MockRuntime>>().open(
            "COM1".to_string(),
            9600,
            Some(DataBits::Eight),
            Some(FlowControl::None),
            Some(Parity::None),
            Some(StopBits::One),
            Some(1000),
        ).unwrap();

        // Test buffer clearing
        let result = app.state::<SerialPort<MockRuntime>>().clear_buffer(
            "COM1".to_string(),
            crate::state::ClearBuffer::All,
        );
        assert!(result.is_ok());

        // Test writing and checking buffer
        app.state::<SerialPort<MockRuntime>>().write(
            "COM1".to_string(),
            "Test".to_string(),
        ).unwrap();

        let result = app.state::<SerialPort<MockRuntime>>().bytes_to_read(
            "COM1".to_string(),
        );
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 4);
    }
} 
