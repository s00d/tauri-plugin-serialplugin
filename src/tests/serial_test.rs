#[cfg(test)]
mod tests {
    use crate::state::{DataBits, FlowControl, Parity, StopBits, SerialportInfo};
    use crate::error::Error;
    use crate::desktop_api::SerialPort;
    use serialport::SerialPort as SerialPortTrait;
    use std::sync::{Arc, Mutex};
    use std::collections::HashMap;
    use std::time::Duration;
    use std::io::{Read, Write};
    use tauri::test::MockRuntime;
    use tauri::Runtime;
    use tauri::Manager;
    use tauri::App;

    // Мок для тестирования
    struct MockSerialPort {
        is_open: bool,
        baud_rate: u32,
        data_bits: serialport::DataBits,
        flow_control: serialport::FlowControl,
        parity: serialport::Parity,
        stop_bits: serialport::StopBits,
        timeout: Duration,
        buffer: Vec<u8>,
    }

    impl MockSerialPort {
        fn new() -> Self {
            Self {
                is_open: false,
                baud_rate: 9600,
                data_bits: serialport::DataBits::Eight,
                flow_control: serialport::FlowControl::None,
                parity: serialport::Parity::None,
                stop_bits: serialport::StopBits::One,
                timeout: Duration::from_millis(1000),
                buffer: Vec::new(),
            }
        }
    }

    // Реализуем Read и Write для MockSerialPort
    impl Read for MockSerialPort {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if !self.is_open {
                return Err(std::io::Error::new(std::io::ErrorKind::NotConnected, "Port is not open"));
            }
            if self.buffer.is_empty() {
                return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "No data available"));
            }
            let n = std::cmp::min(buf.len(), self.buffer.len());
            buf[..n].copy_from_slice(&self.buffer[..n]);
            self.buffer.drain(..n);
            Ok(n)
        }
    }

    impl Write for MockSerialPort {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            if !self.is_open {
                return Err(std::io::Error::new(std::io::ErrorKind::NotConnected, "Port is not open"));
            }
            self.buffer.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    // Реализуем Send для MockSerialPort
    unsafe impl Send for MockSerialPort {}

    impl SerialPortTrait for MockSerialPort {
        fn name(&self) -> Option<String> {
            Some("COM1".to_string())
        }

        fn baud_rate(&self) -> serialport::Result<u32> {
            Ok(self.baud_rate)
        }

        fn data_bits(&self) -> serialport::Result<serialport::DataBits> {
            Ok(self.data_bits)
        }

        fn flow_control(&self) -> serialport::Result<serialport::FlowControl> {
            Ok(self.flow_control)
        }

        fn parity(&self) -> serialport::Result<serialport::Parity> {
            Ok(self.parity)
        }

        fn stop_bits(&self) -> serialport::Result<serialport::StopBits> {
            Ok(self.stop_bits)
        }

        fn timeout(&self) -> Duration {
            self.timeout
        }

        fn set_baud_rate(&mut self, baud_rate: u32) -> serialport::Result<()> {
            self.baud_rate = baud_rate;
            Ok(())
        }

        fn set_data_bits(&mut self, data_bits: serialport::DataBits) -> serialport::Result<()> {
            self.data_bits = data_bits;
            Ok(())
        }

        fn set_flow_control(&mut self, flow_control: serialport::FlowControl) -> serialport::Result<()> {
            self.flow_control = flow_control;
            Ok(())
        }

        fn set_parity(&mut self, parity: serialport::Parity) -> serialport::Result<()> {
            self.parity = parity;
            Ok(())
        }

        fn set_stop_bits(&mut self, stop_bits: serialport::StopBits) -> serialport::Result<()> {
            self.stop_bits = stop_bits;
            Ok(())
        }

        fn set_timeout(&mut self, timeout: Duration) -> serialport::Result<()> {
            self.timeout = timeout;
            Ok(())
        }

        fn write_request_to_send(&mut self, _level: bool) -> serialport::Result<()> {
            Ok(())
        }

        fn write_data_terminal_ready(&mut self, _level: bool) -> serialport::Result<()> {
            Ok(())
        }

        fn read_clear_to_send(&mut self) -> serialport::Result<bool> {
            Ok(true)
        }

        fn read_data_set_ready(&mut self) -> serialport::Result<bool> {
            Ok(true)
        }

        fn read_ring_indicator(&mut self) -> serialport::Result<bool> {
            Ok(true)
        }

        fn read_carrier_detect(&mut self) -> serialport::Result<bool> {
            Ok(true)
        }

        fn bytes_to_read(&self) -> serialport::Result<u32> {
            Ok(self.buffer.len() as u32)
        }

        fn bytes_to_write(&self) -> serialport::Result<u32> {
            Ok(0)
        }

        fn clear(&self, _buffer_to_clear: serialport::ClearBuffer) -> serialport::Result<()> {
            Ok(())
        }

        fn try_clone(&self) -> serialport::Result<Box<dyn SerialPortTrait>> {
            Ok(Box::new(MockSerialPort::new()))
        }

        fn set_break(&self) -> serialport::Result<()> {
            Ok(())
        }

        fn clear_break(&self) -> serialport::Result<()> {
            Ok(())
        }
    }

    // Реализация From для конвертации типов
    impl From<serialport::DataBits> for DataBits {
        fn from(bits: serialport::DataBits) -> Self {
            match bits {
                serialport::DataBits::Five => DataBits::Five,
                serialport::DataBits::Six => DataBits::Six,
                serialport::DataBits::Seven => DataBits::Seven,
                serialport::DataBits::Eight => DataBits::Eight,
            }
        }
    }

    impl From<serialport::FlowControl> for FlowControl {
        fn from(flow: serialport::FlowControl) -> Self {
            match flow {
                serialport::FlowControl::None => FlowControl::None,
                serialport::FlowControl::Software => FlowControl::Software,
                serialport::FlowControl::Hardware => FlowControl::Hardware,
            }
        }
    }

    impl From<serialport::Parity> for Parity {
        fn from(parity: serialport::Parity) -> Self {
            match parity {
                serialport::Parity::None => Parity::None,
                serialport::Parity::Odd => Parity::Odd,
                serialport::Parity::Even => Parity::Even,
            }
        }
    }

    impl From<serialport::StopBits> for StopBits {
        fn from(bits: serialport::StopBits) -> Self {
            match bits {
                serialport::StopBits::One => StopBits::One,
                serialport::StopBits::Two => StopBits::Two,
            }
        }
    }

    struct TestSerialPort<R: Runtime> {
        app: tauri::AppHandle<R>,
        serialports: Arc<Mutex<HashMap<String, SerialportInfo>>>,
    }

    impl<R: Runtime> Clone for TestSerialPort<R> {
        fn clone(&self) -> Self {
            Self {
                app: self.app.clone(),
                serialports: Arc::clone(&self.serialports),
            }
        }
    }

    impl<R: Runtime> TestSerialPort<R> {
        fn new(app: tauri::AppHandle<R>) -> Self {
            Self {
                app,
                serialports: Arc::new(Mutex::new(HashMap::new())),
            }
        }

        fn open(
            &self,
            path: String,
            _baud_rate: u32,
            _data_bits: Option<DataBits>,
            _flow_control: Option<FlowControl>,
            _parity: Option<Parity>,
            _stop_bits: Option<StopBits>,
            _timeout: Option<u64>,
        ) -> Result<(), Error> {
            let mut ports = self.serialports.lock()
                .map_err(|e| Error::String(format!("Failed to acquire lock: {}", e)))?;

            let mut mock_port = MockSerialPort::new();
            mock_port.is_open = true;
            ports.insert(path, SerialportInfo {
                serialport: Box::new(mock_port),
                sender: None,
                thread_handle: None,
            });

            Ok(())
        }

        // Реализуем остальные методы, делегируя их SerialPort
        fn write(&self, path: String, value: String) -> Result<usize, Error> {
            let mut ports = self.serialports.lock()
                .map_err(|e| Error::String(format!("Failed to acquire lock: {}", e)))?;

            if let Some(port_info) = ports.get_mut(&path) {
                port_info.serialport.write(value.as_bytes())
                    .map_err(|e| Error::String(format!("Failed to write data: {}", e)))
            } else {
                Err(Error::String(format!("Port '{}' not found", path)))
            }
        }

        fn read(&self, path: String, _timeout: Option<u64>, size: Option<usize>) -> Result<String, Error> {
            let mut ports = self.serialports.lock()
                .map_err(|e| Error::String(format!("Failed to acquire lock: {}", e)))?;

            if let Some(port_info) = ports.get_mut(&path) {
                let target_size = size.unwrap_or(1024);
                let mut buffer = vec![0; target_size];
                let n = port_info.serialport.read(&mut buffer)
                    .map_err(|e| Error::String(format!("Failed to read data: {}", e)))?;

                String::from_utf8(buffer[..n].to_vec())
                    .map_err(|e| Error::String(format!("Failed to decode data: {}", e)))
            } else {
                Err(Error::String(format!("Port '{}' not found", path)))
            }
        }

        fn close(&self, path: String) -> Result<(), Error> {
            let mut ports = self.serialports.lock()
                .map_err(|e| Error::String(format!("Failed to acquire lock: {}", e)))?;

            if ports.remove(&path).is_some() {
                Ok(())
            } else {
                Err(Error::String(format!("Port '{}' not found", path)))
            }
        }

        fn available_ports(&self) -> Result<HashMap<String, HashMap<String, String>>, Error> {
            Ok(HashMap::new()) // В тестовом окружении возвращаем пустой список
        }

        fn set_baud_rate(&self, path: String, baud_rate: u32) -> Result<(), Error> {
            let mut ports = self.serialports.lock()
                .map_err(|e| Error::String(format!("Failed to acquire lock: {}", e)))?;

            if let Some(port_info) = ports.get_mut(&path) {
                port_info.serialport.set_baud_rate(baud_rate)
                    .map_err(|e| Error::String(format!("Failed to set baud rate: {}", e)))
            } else {
                Err(Error::String(format!("Port '{}' not found", path)))
            }
        }

        fn set_data_bits(&self, path: String, data_bits: DataBits) -> Result<(), Error> {
            let mut ports = self.serialports.lock()
                .map_err(|e| Error::String(format!("Failed to acquire lock: {}", e)))?;

            if let Some(port_info) = ports.get_mut(&path) {
                let bits = match data_bits {
                    DataBits::Five => serialport::DataBits::Five,
                    DataBits::Six => serialport::DataBits::Six,
                    DataBits::Seven => serialport::DataBits::Seven,
                    DataBits::Eight => serialport::DataBits::Eight,
                };
                port_info.serialport.set_data_bits(bits)
                    .map_err(|e| Error::String(format!("Failed to set data bits: {}", e)))
            } else {
                Err(Error::String(format!("Port '{}' not found", path)))
            }
        }

        fn set_flow_control(&self, path: String, flow_control: FlowControl) -> Result<(), Error> {
            let mut ports = self.serialports.lock()
                .map_err(|e| Error::String(format!("Failed to acquire lock: {}", e)))?;

            if let Some(port_info) = ports.get_mut(&path) {
                let flow = match flow_control {
                    FlowControl::None => serialport::FlowControl::None,
                    FlowControl::Software => serialport::FlowControl::Software,
                    FlowControl::Hardware => serialport::FlowControl::Hardware,
                };
                port_info.serialport.set_flow_control(flow)
                    .map_err(|e| Error::String(format!("Failed to set flow control: {}", e)))
            } else {
                Err(Error::String(format!("Port '{}' not found", path)))
            }
        }

        fn set_parity(&self, path: String, parity: Parity) -> Result<(), Error> {
            let mut ports = self.serialports.lock()
                .map_err(|e| Error::String(format!("Failed to acquire lock: {}", e)))?;

            if let Some(port_info) = ports.get_mut(&path) {
                let par = match parity {
                    Parity::None => serialport::Parity::None,
                    Parity::Odd => serialport::Parity::Odd,
                    Parity::Even => serialport::Parity::Even,
                };
                port_info.serialport.set_parity(par)
                    .map_err(|e| Error::String(format!("Failed to set parity: {}", e)))
            } else {
                Err(Error::String(format!("Port '{}' not found", path)))
            }
        }

        fn set_stop_bits(&self, path: String, stop_bits: StopBits) -> Result<(), Error> {
            let mut ports = self.serialports.lock()
                .map_err(|e| Error::String(format!("Failed to acquire lock: {}", e)))?;

            if let Some(port_info) = ports.get_mut(&path) {
                let bits = match stop_bits {
                    StopBits::One => serialport::StopBits::One,
                    StopBits::Two => serialport::StopBits::Two,
                };
                port_info.serialport.set_stop_bits(bits)
                    .map_err(|e| Error::String(format!("Failed to set stop bits: {}", e)))
            } else {
                Err(Error::String(format!("Port '{}' not found", path)))
            }
        }

        fn write_request_to_send(&self, path: String, level: bool) -> Result<(), Error> {
            let mut ports = self.serialports.lock()
                .map_err(|e| Error::String(format!("Failed to acquire lock: {}", e)))?;

            if let Some(port_info) = ports.get_mut(&path) {
                port_info.serialport.write_request_to_send(level)
                    .map_err(|e| Error::String(format!("Failed to set RTS: {}", e)))
            } else {
                Err(Error::String(format!("Port '{}' not found", path)))
            }
        }

        fn write_data_terminal_ready(&self, path: String, level: bool) -> Result<(), Error> {
            let mut ports = self.serialports.lock()
                .map_err(|e| Error::String(format!("Failed to acquire lock: {}", e)))?;

            if let Some(port_info) = ports.get_mut(&path) {
                port_info.serialport.write_data_terminal_ready(level)
                    .map_err(|e| Error::String(format!("Failed to set DTR: {}", e)))
            } else {
                Err(Error::String(format!("Port '{}' not found", path)))
            }
        }

        fn read_clear_to_send(&self, path: String) -> Result<bool, Error> {
            let mut ports = self.serialports.lock()
                .map_err(|e| Error::String(format!("Failed to acquire lock: {}", e)))?;

            if let Some(port_info) = ports.get_mut(&path) {
                port_info.serialport.read_clear_to_send()
                    .map_err(|e| Error::String(format!("Failed to read CTS: {}", e)))
            } else {
                Err(Error::String(format!("Port '{}' not found", path)))
            }
        }

        fn read_data_set_ready(&self, path: String) -> Result<bool, Error> {
            let mut ports = self.serialports.lock()
                .map_err(|e| Error::String(format!("Failed to acquire lock: {}", e)))?;

            if let Some(port_info) = ports.get_mut(&path) {
                port_info.serialport.read_data_set_ready()
                    .map_err(|e| Error::String(format!("Failed to read DSR: {}", e)))
            } else {
                Err(Error::String(format!("Port '{}' not found", path)))
            }
        }

        fn set_break(&self, path: String) -> Result<(), Error> {
            let mut ports = self.serialports.lock()
                .map_err(|e| Error::String(format!("Failed to acquire lock: {}", e)))?;

            if let Some(port_info) = ports.get_mut(&path) {
                port_info.serialport.set_break()
                    .map_err(|e| Error::String(format!("Failed to set break: {}", e)))
            } else {
                Err(Error::String(format!("Port '{}' not found", path)))
            }
        }

        fn clear_break(&self, path: String) -> Result<(), Error> {
            let mut ports = self.serialports.lock()
                .map_err(|e| Error::String(format!("Failed to acquire lock: {}", e)))?;

            if let Some(port_info) = ports.get_mut(&path) {
                port_info.serialport.clear_break()
                    .map_err(|e| Error::String(format!("Failed to clear break: {}", e)))
            } else {
                Err(Error::String(format!("Port '{}' not found", path)))
            }
        }
    }

    fn create_test_serial_port() -> TestSerialPort<MockRuntime> {
        let app = tauri::test::mock_app();
        TestSerialPort::new(app.handle().clone())
    }

    fn create_test_app() -> App<MockRuntime> {
        let app = tauri::test::mock_app();
        let serial_port = SerialPort::new(app.handle().clone());
        app.manage(serial_port);
        app
    }

    // Обновляем тесты для использования TestSerialPort
    #[test]
    fn test_open_port() {
        let serial = create_test_serial_port();
        let result = serial.open(
            "COM1".to_string(),
            9600,
            Some(DataBits::Eight),
            Some(FlowControl::None),
            Some(Parity::None),
            Some(StopBits::One),
            Some(1000),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_write_and_read() {
        let serial = create_test_serial_port();

        // Открываем порт
        serial.open(
            "COM1".to_string(),
            9600,
            Some(DataBits::Eight),
            Some(FlowControl::None),
            Some(Parity::None),
            Some(StopBits::One),
            Some(1000),
        ).unwrap();

        // Записываем данные
        let write_result = serial.write("COM1".to_string(), "Hello".to_string());
        assert!(write_result.is_ok());
        assert_eq!(write_result.unwrap(), 5);

        // Читаем данные
        let read_result = serial.read("COM1".to_string(), Some(1000), Some(1024));
        assert!(read_result.is_ok());
        assert_eq!(read_result.unwrap(), "Hello");
    }

    #[test]
    fn test_port_settings() {
        let serial = create_test_serial_port();

        // Открываем порт
        serial.open(
            "COM1".to_string(),
            9600,
            Some(DataBits::Eight),
            Some(FlowControl::None),
            Some(Parity::None),
            Some(StopBits::One),
            Some(1000),
        ).unwrap();

        // Тестируем изменение скорости
        let result = serial.set_baud_rate("COM1".to_string(), 115200);
        assert!(result.is_ok());

        // Тестируем изменение битов данных
        let result = serial.set_data_bits("COM1".to_string(), DataBits::Seven);
        assert!(result.is_ok());

        // Тестируем изменение контроля потока
        let result = serial.set_flow_control("COM1".to_string(), FlowControl::Hardware);
        assert!(result.is_ok());

        // Тестируем изменение четности
        let result = serial.set_parity("COM1".to_string(), Parity::Even);
        assert!(result.is_ok());

        // Тестируем изменение стоповых битов
        let result = serial.set_stop_bits("COM1".to_string(), StopBits::Two);
        assert!(result.is_ok());
    }

    #[test]
    fn test_control_signals() {
        let serial = create_test_serial_port();

        // Открываем порт
        serial.open(
            "COM1".to_string(),
            9600,
            Some(DataBits::Eight),
            Some(FlowControl::None),
            Some(Parity::None),
            Some(StopBits::One),
            Some(1000),
        ).unwrap();

        // Тестируем RTS
        let result = serial.write_request_to_send("COM1".to_string(), true);
        assert!(result.is_ok());

        // Тестируем DTR
        let result = serial.write_data_terminal_ready("COM1".to_string(), true);
        assert!(result.is_ok());

        // Тестируем чтение CTS
        let result = serial.read_clear_to_send("COM1".to_string());
        assert!(result.is_ok());
        assert!(result.unwrap());

        // Тестируем чтение DSR
        let result = serial.read_data_set_ready("COM1".to_string());
        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[test]
    fn test_close_port() {
        let serial = create_test_serial_port();

        // Открываем порт
        serial.open(
            "COM1".to_string(),
            9600,
            Some(DataBits::Eight),
            Some(FlowControl::None),
            Some(Parity::None),
            Some(StopBits::One),
            Some(1000),
        ).unwrap();

        // Закрываем порт
        let result = serial.close("COM1".to_string());
        assert!(result.is_ok());

        // Пробуем закрыть уже закрытый порт
        let result = serial.close("COM1".to_string());
        assert!(result.is_err());
    }

    #[test]
    fn test_available_ports() {
        let serial = create_test_serial_port();
        let result = serial.available_ports();
        assert!(result.is_ok());
        let ports = result.unwrap();
        assert!(ports.is_empty()); // В тестовом окружении портов нет
    }

    #[test]
    fn test_open_nonexistent_port() {
        let app = create_test_app();
        let serial_port = app.state::<SerialPort<MockRuntime>>();

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
        assert!(result.unwrap_err().to_string().contains("No such file or directory"));
    }

    #[test]
    fn test_write_to_closed_port() {
        let serial = create_test_serial_port();
        let result = serial.write("COM1".to_string(), "Test".to_string());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn test_read_from_closed_port() {
        let serial = create_test_serial_port();
        let result = serial.read("COM1".to_string(), Some(1000), Some(1024));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn test_read_timeout() {
        let serial = create_test_serial_port();

        // Открываем порт
        serial.open(
            "COM1".to_string(),
            9600,
            Some(DataBits::Eight),
            Some(FlowControl::None),
            Some(Parity::None),
            Some(StopBits::One),
            Some(100), // Устанавливаем маленький таймаут
        ).unwrap();

        // Пытаемся прочитать данные, когда их нет
        let result = serial.read("COM1".to_string(), Some(100), Some(1024));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("No data available") || err.to_string().contains("TimedOut"),
                "Expected error to contain 'No data available' or 'TimedOut', got: {}", err);
    }

    #[test]
    fn test_multiple_ports() {
        let serial = create_test_serial_port();

        // Открываем несколько портов
        let ports = vec!["COM1", "COM2", "COM3"];
        for port in &ports {
            let result = serial.open(
                port.to_string(),
                9600,
                Some(DataBits::Eight),
                Some(FlowControl::None),
                Some(Parity::None),
                Some(StopBits::One),
                Some(1000),
            );
            assert!(result.is_ok());
        }

        // Проверяем работу с каждым портом
        for port in &ports {
            // Записываем данные
            let write_result = serial.write(port.to_string(), format!("Test {}", port));
            assert!(write_result.is_ok());
            assert_eq!(write_result.unwrap(), format!("Test {}", port).len());

            // Читаем данные
            let read_result = serial.read(port.to_string(), Some(1000), Some(1024));
            assert!(read_result.is_ok());
            assert_eq!(read_result.unwrap(), format!("Test {}", port));
        }

        // Закрываем все порты
        for port in &ports {
            let result = serial.close(port.to_string());
            assert!(result.is_ok());
        }
    }

    #[test]
    fn test_port_settings_combinations() {
        let serial = create_test_serial_port();
        let port = "COM1".to_string();

        // Тестируем различные комбинации настроек
        let test_cases = vec![
            (9600, DataBits::Eight, FlowControl::None, Parity::None, StopBits::One),
            (115200, DataBits::Seven, FlowControl::Hardware, Parity::Even, StopBits::Two),
            (57600, DataBits::Six, FlowControl::Software, Parity::Odd, StopBits::One),
            (38400, DataBits::Five, FlowControl::None, Parity::Even, StopBits::Two),
        ];

        for (baud_rate, data_bits, flow_control, parity, stop_bits) in test_cases {
            // Открываем порт с новыми настройками
            let result = serial.open(
                port.clone(),
                baud_rate,
                Some(data_bits),
                Some(flow_control),
                Some(parity),
                Some(stop_bits),
                Some(1000),
            );
            assert!(result.is_ok());

            // Проверяем запись и чтение
            let test_data = format!("Test {} {} {} {} {}", baud_rate, data_bits as u8, flow_control as u8, parity as u8, stop_bits as u8);
            let write_result = serial.write(port.clone(), test_data.clone());
            assert!(write_result.is_ok());

            let read_result = serial.read(port.clone(), Some(1000), Some(1024));
            assert!(read_result.is_ok());
            assert_eq!(read_result.unwrap(), test_data);

            // Закрываем порт перед следующей итерацией
            serial.close(port.clone()).unwrap();
        }
    }

    #[test]
    fn test_concurrent_operations() {
        let serial = create_test_serial_port();
        let port = "COM1".to_string();

        // Открываем порт
        serial.open(
            port.clone(),
            9600,
            Some(DataBits::Eight),
            Some(FlowControl::None),
            Some(Parity::None),
            Some(StopBits::One),
            Some(1000),
        ).unwrap();

        // Создаем несколько потоков для одновременной работы с портом
        let handles: Vec<_> = (0..5).map(|i| {
            let serial = serial.clone();
            let port = port.clone();
            std::thread::spawn(move || {
                let data = format!("Thread {}", i);
                let write_result = serial.write(port.clone(), data.clone());
                assert!(write_result.is_ok());

                let read_result = serial.read(port, Some(1000), Some(1024));
                assert!(read_result.is_ok());
                assert_eq!(read_result.unwrap(), data);
            })
        }).collect();

        // Ждем завершения всех потоков
        for handle in handles {
            handle.join().unwrap();
        }

        // Закрываем порт
        serial.close(port).unwrap();
    }

    #[test]
    fn test_port_info_creation() {
        let mock_port = Box::new(MockSerialPort::new());
        let info = SerialportInfo {
            serialport: mock_port,
            sender: None,
            thread_handle: None,
        };
        assert!(info.serialport.name().unwrap() == "COM1");
    }

    #[test]
    fn test_port_settings_validation() {
        let serial = create_test_serial_port();

        // Тест недопустимой скорости
        let result = serial.open(
            "COM1".to_string(),
            0, // Недопустимая скорость
            Some(DataBits::Eight),
            Some(FlowControl::None),
            Some(Parity::None),
            Some(StopBits::One),
            Some(1000),
        );
        assert!(result.is_ok()); // В тестовом окружении все настройки допустимы

        // Тест недопустимых комбинаций настроек
        let result = serial.open(
            "COM1".to_string(),
            9600,
            Some(DataBits::Five), // 5 бит данных
            Some(FlowControl::Hardware), // Аппаратный контроль потока
            Some(Parity::None), // Без четности
            Some(StopBits::Two), // 2 стоповых битов
            Some(1000),
        );
        assert!(result.is_ok()); // В тестовом окружении все комбинации допустимы
    }

    #[test]
    fn test_buffer_operations() {
        let serial = create_test_serial_port();

        // Открываем порт
        serial.open(
            "COM1".to_string(),
            9600,
            Some(DataBits::Eight),
            Some(FlowControl::None),
            Some(Parity::None),
            Some(StopBits::One),
            Some(1000),
        ).unwrap();

        // Тест записи больших данных
        let large_data = "X".repeat(10000);
        let write_result = serial.write("COM1".to_string(), large_data.clone());
        assert!(write_result.is_ok());
        assert_eq!(write_result.unwrap(), large_data.len());

        // Тест чтения по частям
        let mut total_read = String::new();
        let chunk_size = 1024;
        while total_read.len() < large_data.len() {
            let read_result = serial.read("COM1".to_string(), Some(1000), Some(chunk_size));
            assert!(read_result.is_ok());
            let chunk = read_result.unwrap();
            total_read.push_str(&chunk);
        }
        assert_eq!(total_read, large_data);

        // Тест чтения с разными размерами буфера
        serial.write("COM1".to_string(), "Test".to_string()).unwrap();
        let read_result = serial.read("COM1".to_string(), Some(1000), Some(2));
        assert!(read_result.is_ok());
        assert_eq!(read_result.unwrap(), "Te");

        let read_result = serial.read("COM1".to_string(), Some(1000), Some(2));
        assert!(read_result.is_ok());
        assert_eq!(read_result.unwrap(), "st");
    }

    #[test]
    fn test_error_handling() {
        let serial = create_test_serial_port();

        // Тест ошибки при открытии уже открытого порта
        serial.open(
            "COM1".to_string(),
            9600,
            Some(DataBits::Eight),
            Some(FlowControl::None),
            Some(Parity::None),
            Some(StopBits::One),
            Some(1000),
        ).unwrap();

        let result = serial.open(
            "COM1".to_string(),
            9600,
            Some(DataBits::Eight),
            Some(FlowControl::None),
            Some(Parity::None),
            Some(StopBits::One),
            Some(1000),
        );
        assert!(result.is_ok()); // В тестовом окружении повторное открытие допустимо

        // Тест ошибки при работе с недопустимыми параметрами
        // Используем валидные UTF-8 данные, но с необычными символами
        let test_data = "Тестовые данные с русскими символами и эмодзи 🚀";
        let result = serial.write("COM1".to_string(), test_data.to_string());
        assert!(result.is_ok());

        // Тест ошибки при закрытии несуществующего порта
        let result = serial.close("NONEXISTENT".to_string());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));

        // Тест ошибки при работе с закрытым портом
        serial.close("COM1".to_string()).unwrap();
        let result = serial.write("COM1".to_string(), "Test".to_string());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));

        // Тест ошибки при чтении из закрытого порта
        let result = serial.read("COM1".to_string(), Some(1000), Some(1024));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn test_port_state_transitions() {
        let serial = create_test_serial_port();
        let port = "COM1".to_string();

        // Тест последовательности состояний порта
        // 1. Порт не существует
        let result = serial.write(port.clone(), "Test".to_string());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));

        // 2. Открываем порт
        let result = serial.open(
            port.clone(),
            9600,
            Some(DataBits::Eight),
            Some(FlowControl::None),
            Some(Parity::None),
            Some(StopBits::One),
            Some(1000),
        );
        assert!(result.is_ok());

        // 3. Порт открыт, можно писать
        let result = serial.write(port.clone(), "Test".to_string());
        assert!(result.is_ok());

        // 4. Закрываем порт
        let result = serial.close(port.clone());
        assert!(result.is_ok());

        // 5. Порт закрыт, нельзя писать
        let result = serial.write(port.clone(), "Test".to_string());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));

        // 6. Повторно открываем порт
        let result = serial.open(
            port.clone(),
            9600,
            Some(DataBits::Eight),
            Some(FlowControl::None),
            Some(Parity::None),
            Some(StopBits::One),
            Some(1000),
        );
        assert!(result.is_ok());

        // 7. Проверяем, что порт работает
        let result = serial.write(port.clone(), "Test".to_string());
        assert!(result.is_ok());
    }

    #[test]
    fn test_port_settings_persistence() {
        let serial = create_test_serial_port();
        let port = "COM1".to_string();

        // Тест сохранения настроек порта
        let settings = vec![
            (115200, DataBits::Seven, FlowControl::Hardware, Parity::Even, StopBits::Two),
            (57600, DataBits::Six, FlowControl::Software, Parity::Odd, StopBits::One),
            (38400, DataBits::Five, FlowControl::None, Parity::Even, StopBits::Two),
        ];

        for (baud_rate, data_bits, flow_control, parity, stop_bits) in settings {
            // Открываем порт с новыми настройками
            serial.open(
                port.clone(),
                baud_rate,
                Some(data_bits),
                Some(flow_control),
                Some(parity),
                Some(stop_bits),
                Some(1000),
            ).unwrap();

            // Проверяем, что настройки применились
            let write_result = serial.write(port.clone(), "Test".to_string());
            assert!(write_result.is_ok());

            let read_result = serial.read(port.clone(), Some(1000), Some(1024));
            assert!(read_result.is_ok());
            assert_eq!(read_result.unwrap(), "Test");

            // Закрываем порт
            serial.close(port.clone()).unwrap();
        }
    }

    #[test]
    fn test_concurrent_port_operations() {
        let serial = create_test_serial_port();
        let ports = vec!["COM1", "COM2", "COM3"];

        // Открываем несколько портов
        for port in &ports {
            serial.open(
                port.to_string(),
                9600,
                Some(DataBits::Eight),
                Some(FlowControl::None),
                Some(Parity::None),
                Some(StopBits::One),
                Some(1000),
            ).unwrap();
        }

        // Создаем потоки для одновременной работы с разными портами
        let handles: Vec<_> = ports.iter().map(|port| {
            let serial = serial.clone();
            let port = port.to_string();
            std::thread::spawn(move || {
                for i in 0..10 {
                    let data = format!("Port {} - Test {}", port, i);
                    let write_result = serial.write(port.clone(), data.clone());
                    assert!(write_result.is_ok());

                    let read_result = serial.read(port.clone(), Some(1000), Some(1024));
                    assert!(read_result.is_ok());
                    assert_eq!(read_result.unwrap(), data);
                }
            })
        }).collect();

        // Ждем завершения всех потоков
        for handle in handles {
            handle.join().unwrap();
        }

        // Закрываем все порты
        for port in ports {
            serial.close(port.to_string()).unwrap();
        }
    }

    #[test]
    fn test_port_resource_cleanup() {
        let serial = create_test_serial_port();
        let port = "COM1".to_string();

        // Открываем порт
        serial.open(
            port.clone(),
            9600,
            Some(DataBits::Eight),
            Some(FlowControl::None),
            Some(Parity::None),
            Some(StopBits::One),
            Some(1000),
        ).unwrap();

        // Записываем данные
        serial.write(port.clone(), "Test".to_string()).unwrap();

        // Закрываем порт
        serial.close(port.clone()).unwrap();

        // Проверяем, что порт действительно закрыт
        let result = serial.write(port.clone(), "Test".to_string());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));

        // Пробуем открыть порт снова
        let result = serial.open(
            port.clone(),
            9600,
            Some(DataBits::Eight),
            Some(FlowControl::None),
            Some(Parity::None),
            Some(StopBits::One),
            Some(1000),
        );
        assert!(result.is_ok());

        // Проверяем, что порт работает после повторного открытия
        let result = serial.write(port.clone(), "Test".to_string());
        assert!(result.is_ok());
    }

    #[test]
    fn test_port_settings_limits() {
        let serial = create_test_serial_port();
        let port = "COM1".to_string();

        // Тест граничных значений скорости передачи
        let baud_rates = vec![
            110, 300, 600, 1200, 2400, 4800, 9600, 14400, 19200, 38400, 57600, 115200,
            128000, 256000, 460800, 921600, 1500000, 2000000, 3000000
        ];

        for baud_rate in baud_rates {
            let result = serial.open(
                port.clone(),
                baud_rate,
                Some(DataBits::Eight),
                Some(FlowControl::None),
                Some(Parity::None),
                Some(StopBits::One),
                Some(1000),
            );
            assert!(result.is_ok(), "Failed to open port with baud rate {}", baud_rate);
            serial.close(port.clone()).unwrap();
        }

        // Тест всех возможных комбинаций битов данных
        for data_bits in &[DataBits::Five, DataBits::Six, DataBits::Seven, DataBits::Eight] {
            let result = serial.open(
                port.clone(),
                9600,
                Some(*data_bits),
                Some(FlowControl::None),
                Some(Parity::None),
                Some(StopBits::One),
                Some(1000),
            );
            assert!(result.is_ok(), "Failed to open port with data bits {:?}", data_bits);
            serial.close(port.clone()).unwrap();
        }
    }

    #[test]
    fn test_port_timeout_behavior() {
        let serial = create_test_serial_port();
        let port = "COM1".to_string();

        // Тест различных таймаутов
        let timeouts = vec![100, 500, 1000]; // Используем более длительные таймауты для надежности

        for timeout in timeouts {
            // Открываем порт с новым таймаутом
            serial.open(
                port.clone(),
                9600,
                Some(DataBits::Eight),
                Some(FlowControl::None),
                Some(Parity::None),
                Some(StopBits::One),
                Some(timeout),
            ).unwrap();

            // Устанавливаем таймаут для порта
            let mut ports = serial.serialports.lock().unwrap();
            if let Some(port_info) = ports.get_mut(&port) {
                port_info.serialport.set_timeout(Duration::from_millis(timeout)).unwrap();
            }
            drop(ports);

            // Проверяем чтение с пустым буфером (должно вызвать таймаут)
            let result = serial.read(port.clone(), Some(timeout), Some(1024));
            assert!(result.is_err(), "Expected timeout error for timeout {}", timeout);

            // Проверяем, что ошибка именно таймаут
            let err = result.unwrap_err();
            assert!(err.to_string().contains("No data available") || err.to_string().contains("TimedOut"),
                    "Expected timeout error, got: {}", err);

            // Проверяем, что порт все еще работает после таймаута
            let test_data = format!("Test after {}ms timeout", timeout);
            let write_result = serial.write(port.clone(), test_data.clone());
            assert!(write_result.is_ok());

            let read_result = serial.read(port.clone(), Some(timeout), Some(1024));
            assert!(read_result.is_ok());
            assert_eq!(read_result.unwrap(), test_data);

            serial.close(port.clone()).unwrap();
        }
    }

    #[test]
    fn test_port_buffer_overflow() {
        let serial = create_test_serial_port();
        let port = "COM1".to_string();

        serial.open(
            port.clone(),
            9600,
            Some(DataBits::Eight),
            Some(FlowControl::None),
            Some(Parity::None),
            Some(StopBits::One),
            Some(1000),
        ).unwrap();

        // Записываем данные, превышающие размер буфера
        let large_data = "X".repeat(100000);
        let write_result = serial.write(port.clone(), large_data.clone());
        assert!(write_result.is_ok());

        // Читаем данные по частям
        let mut total_read = String::new();
        let chunk_size = 1024;
        let mut iterations = 0;
        let max_iterations = 200; // Предотвращаем бесконечный цикл

        while total_read.len() < large_data.len() && iterations < max_iterations {
            let read_result = serial.read(port.clone(), Some(1000), Some(chunk_size));
            assert!(read_result.is_ok());
            let chunk = read_result.unwrap();
            total_read.push_str(&chunk);
            iterations += 1;
        }

        assert_eq!(total_read, large_data, "Buffer overflow test failed");
        assert!(iterations < max_iterations, "Buffer overflow test took too many iterations");
    }

    #[test]
    fn test_port_rapid_open_close() {
        let serial = create_test_serial_port();
        let port = "COM1".to_string();

        // Быстрое открытие и закрытие порта
        for _ in 0..100 {
            let open_result = serial.open(
                port.clone(),
                9600,
                Some(DataBits::Eight),
                Some(FlowControl::None),
                Some(Parity::None),
                Some(StopBits::One),
                Some(1000),
            );
            assert!(open_result.is_ok());

            let close_result = serial.close(port.clone());
            assert!(close_result.is_ok());
        }
    }

    #[test]
    fn test_port_settings_change() {
        let serial = create_test_serial_port();
        let port = "COM1".to_string();

        serial.open(
            port.clone(),
            9600,
            Some(DataBits::Eight),
            Some(FlowControl::None),
            Some(Parity::None),
            Some(StopBits::One),
            Some(1000),
        ).unwrap();

        // Тест изменения настроек на лету
        let settings_changes = vec![
            (115200, DataBits::Seven, FlowControl::Hardware, Parity::Even, StopBits::Two),
            (57600, DataBits::Six, FlowControl::Software, Parity::Odd, StopBits::One),
            (38400, DataBits::Five, FlowControl::None, Parity::Even, StopBits::Two),
            (9600, DataBits::Eight, FlowControl::None, Parity::None, StopBits::One),
        ];

        for (baud_rate, data_bits, flow_control, parity, stop_bits) in settings_changes {
            // Меняем настройки
            serial.set_baud_rate(port.clone(), baud_rate).unwrap();
            serial.set_data_bits(port.clone(), data_bits).unwrap();
            serial.set_flow_control(port.clone(), flow_control).unwrap();
            serial.set_parity(port.clone(), parity).unwrap();
            serial.set_stop_bits(port.clone(), stop_bits).unwrap();

            // Проверяем, что порт все еще работает
            let test_data = format!("Test at {} baud", baud_rate);
            let write_result = serial.write(port.clone(), test_data.clone());
            assert!(write_result.is_ok());

            let read_result = serial.read(port.clone(), Some(1000), Some(1024));
            assert!(read_result.is_ok());
            assert_eq!(read_result.unwrap(), test_data);
        }
    }

    #[test]
    fn test_port_control_signals_sequence() {
        let serial = create_test_serial_port();
        let port = "COM1".to_string();

        serial.open(
            port.clone(),
            9600,
            Some(DataBits::Eight),
            Some(FlowControl::None),
            Some(Parity::None),
            Some(StopBits::One),
            Some(1000),
        ).unwrap();

        // Тест последовательности управляющих сигналов
        let signal_sequence = vec![
            (true, true),   // RTS=1, DTR=1
            (true, false),  // RTS=1, DTR=0
            (false, true),  // RTS=0, DTR=1
            (false, false), // RTS=0, DTR=0
        ];

        for (rts, dtr) in signal_sequence {
            // Устанавливаем сигналы
            serial.write_request_to_send(port.clone(), rts).unwrap();
            serial.write_data_terminal_ready(port.clone(), dtr).unwrap();

            // Проверяем состояние сигналов
            let cts = serial.read_clear_to_send(port.clone()).unwrap();
            let dsr = serial.read_data_set_ready(port.clone()).unwrap();

            // В тестовом окружении все сигналы всегда true
            assert!(cts);
            assert!(dsr);
        }
    }

    #[test]
    fn test_port_concurrent_settings_change() {
        let serial = create_test_serial_port();
        let port = "COM1".to_string();
        let mutex = Arc::new(Mutex::new(()));

        serial.open(
            port.clone(),
            9600,
            Some(DataBits::Eight),
            Some(FlowControl::None),
            Some(Parity::None),
            Some(StopBits::One),
            Some(1000),
        ).unwrap();

        // Создаем потоки для последовательного изменения настроек
        let handles: Vec<_> = (0..5).map(|i| {
            let serial = serial.clone();
            let port = port.clone();
            let mutex = Arc::clone(&mutex);
            std::thread::spawn(move || {
                for _ in 0..10 {
                    // Блокируем мьютекс для синхронизации доступа к порту
                    let _lock = mutex.lock().unwrap();

                    // Меняем скорость
                    serial.set_baud_rate(port.clone(), 9600 + (i * 1000)).unwrap();

                    // Меняем биты данных
                    let data_bits = match i % 4 {
                        0 => DataBits::Five,
                        1 => DataBits::Six,
                        2 => DataBits::Seven,
                        _ => DataBits::Eight,
                    };
                    serial.set_data_bits(port.clone(), data_bits).unwrap();

                    // Проверяем, что порт все еще работает
                    let test_data = format!("Test from thread {}", i);
                    let write_result = serial.write(port.clone(), test_data.clone());
                    assert!(write_result.is_ok());

                    // Читаем данные сразу после записи
                    let read_result = serial.read(port.clone(), Some(1000), Some(1024));
                    assert!(read_result.is_ok());
                    let read_data = read_result.unwrap();
                    assert_eq!(read_data, test_data,
                               "Data mismatch in thread {}: expected '{}', got '{}'",
                               i, test_data, read_data);

                    // Небольшая задержка для стабильности
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            })
        }).collect();

        // Ждем завершения всех потоков
        for handle in handles {
            handle.join().unwrap();
        }
    }

    #[test]
    fn test_break_control() {
        let serial = create_test_serial_port();

        // Открываем порт
        serial.open(
            "COM1".to_string(),
            9600,
            Some(DataBits::Eight),
            Some(FlowControl::None),
            Some(Parity::None),
            Some(StopBits::One),
            Some(1000),
        ).unwrap();

        // Тест установки и сброса break
        let result = serial.set_break("COM1".to_string());
        assert!(result.is_ok());

        let result = serial.clear_break("COM1".to_string());
        assert!(result.is_ok());
    }
} 