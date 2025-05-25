#[cfg(test)]
mod tests {
    use crate::{
        available_ports,
        managed_ports,
        open,
        close,
        write,
        read,
        write_request_to_send,
        write_data_terminal_ready,
        set_baud_rate,
        set_data_bits,
        clear_buffer,
    };
    use crate::state::{DataBits, FlowControl, Parity, StopBits, ClearBuffer};
    use tauri::{App, Manager};
    use crate::desktop_api::SerialPort;
    use tauri::test::MockRuntime;

    fn create_test_app() -> App<MockRuntime> {
        let app = tauri::test::mock_app();
        let serial_port = SerialPort::new(app.handle().clone());
        app.manage(serial_port);
        app
    }

    #[test]
    fn test_available_ports() {
        let app = create_test_app();
        let serial_port = SerialPort::new(app.handle().clone());
        app.manage(serial_port);

        let result = available_ports(app.handle().clone(), app.state::<SerialPort<MockRuntime>>());
        assert!(result.is_ok());
    }

    #[test]
    fn test_managed_ports() {
        let app = create_test_app();
        let serial_port = SerialPort::new(app.handle().clone());
        app.manage(serial_port);

        let result = managed_ports(app.handle().clone(), app.state::<SerialPort<MockRuntime>>());
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Vec::<String>::new());
    }

    #[test]
    fn test_open_close() {
        let app = create_test_app();
        
        // Тест должен ожидать ошибку при открытии несуществующего порта
        let result = open(
            app.handle().clone(),
            app.state::<SerialPort<MockRuntime>>(),
            "NONEXISTENT".to_string(),
            9600,
            Some(DataBits::Eight),
            Some(FlowControl::None),
            Some(Parity::None),
            Some(StopBits::One),
            Some(1000),
        );
        assert!(result.is_err());

        // Тест закрытия несуществующего порта
        let result = close(
            app.handle().clone(),
            app.state::<SerialPort<MockRuntime>>(),
            "NONEXISTENT".to_string(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_write_read() {
        let app = create_test_app();
        
        // Тест должен ожидать ошибку при записи в несуществующий порт
        let result = write(
            app.handle().clone(),
            app.state::<SerialPort<MockRuntime>>(),
            "NONEXISTENT".to_string(),
            "Test data".to_string(),
        );
        assert!(result.is_err());

        // Тест должен ожидать ошибку при чтении из несуществующего порта
        let result = read(
            app.handle().clone(),
            app.state::<SerialPort<MockRuntime>>(),
            "NONEXISTENT".to_string(),
            Some(1000),
            Some(1024),
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_port_settings() {
        let app = create_test_app();
        
        // Тест должен ожидать ошибку при установке настроек несуществующего порта
        let result = set_baud_rate(
            app.handle().clone(),
            app.state::<SerialPort<MockRuntime>>(),
            "NONEXISTENT".to_string(),
            115200,
        );
        assert!(result.is_err());

        let result = set_data_bits(
            app.handle().clone(),
            app.state::<SerialPort<MockRuntime>>(),
            "NONEXISTENT".to_string(),
            DataBits::Seven,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_control_signals() {
        let app = create_test_app();
        
        // Тест должен ожидать ошибку при установке сигналов управления несуществующего порта
        let result = write_request_to_send(
            app.handle().clone(),
            app.state::<SerialPort<MockRuntime>>(),
            "NONEXISTENT".to_string(),
            true,
        );
        assert!(result.is_err());

        let result = write_data_terminal_ready(
            app.handle().clone(),
            app.state::<SerialPort<MockRuntime>>(),
            "NONEXISTENT".to_string(),
            true,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_buffer_operations() {
        let app = create_test_app();
        
        // Тест должен ожидать ошибку при операциях с буфером несуществующего порта
        let result = clear_buffer(
            app.handle().clone(),
            app.state::<SerialPort<MockRuntime>>(),
            "NONEXISTENT".to_string(),
            ClearBuffer::All,
        );
        assert!(result.is_err());
    }
} 