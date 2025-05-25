#[cfg(test)]
mod tests {
    use crate::state::{FlowControl, Parity, SerialportInfo};
    use crate::tests::mock::MockSerialPort;
    use serialport::SerialPort;
    use std::time::Duration;

    #[test]
    fn test_serialport_info() {
        let mock_port = MockSerialPort::new();
        let info = SerialportInfo {
            serialport: Box::new(mock_port),
            sender: None,
            thread_handle: None,
        };

        assert_eq!(info.serialport.name().unwrap(), "COM1");
        assert_eq!(info.serialport.baud_rate().unwrap(), 9600);
        assert_eq!(info.serialport.data_bits().unwrap(), serialport::DataBits::Eight);
        assert_eq!(info.serialport.flow_control().unwrap(), serialport::FlowControl::None);
        assert_eq!(info.serialport.parity().unwrap(), serialport::Parity::None);
        assert_eq!(info.serialport.stop_bits().unwrap(), serialport::StopBits::One);
        assert_eq!(info.serialport.timeout(), Duration::from_millis(1000));
        assert!(info.sender.is_none());
        assert!(info.thread_handle.is_none());
    }

    #[test]
    fn test_data_bits() {
        let mut port = MockSerialPort::new();
        assert_eq!(port.data_bits().unwrap(), serialport::DataBits::Eight);
        port.set_data_bits(serialport::DataBits::Five).unwrap();
        assert_eq!(port.data_bits().unwrap(), serialport::DataBits::Five);
    }

    #[test]
    fn test_flow_control() {
        assert_eq!(FlowControl::None as u8, 0);
        assert_eq!(FlowControl::Software as u8, 1);
        assert_eq!(FlowControl::Hardware as u8, 2);
    }

    #[test]
    fn test_parity() {
        assert_eq!(Parity::None as u8, 0);
        assert_eq!(Parity::Odd as u8, 1);
        assert_eq!(Parity::Even as u8, 2);
    }

    #[test]
    fn test_stop_bits() {
        let mut port = MockSerialPort::new();
        assert_eq!(port.stop_bits().unwrap(), serialport::StopBits::One);
        port.set_stop_bits(serialport::StopBits::Two).unwrap();
        assert_eq!(port.stop_bits().unwrap(), serialport::StopBits::Two);
    }

    #[test]
    fn test_clear_buffer() {
        let port = MockSerialPort::new();
        assert!(port.clear(serialport::ClearBuffer::All).is_ok());
        assert!(port.clear(serialport::ClearBuffer::Input).is_ok());
        assert!(port.clear(serialport::ClearBuffer::Output).is_ok());
    }
} 