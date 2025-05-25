use std::io::{self, Read, Write};
use serialport::{self, SerialPort};
use std::time::Duration;
use crate::state::SerialportInfo;

#[allow(dead_code)]
pub trait SerialPortTrait: Read + Write + Send {
    fn name(&self) -> Option<String>;
    fn baud_rate(&self) -> io::Result<u32>;
    fn data_bits(&self) -> io::Result<serialport::DataBits>;
    fn flow_control(&self) -> io::Result<serialport::FlowControl>;
    fn parity(&self) -> io::Result<serialport::Parity>;
    fn stop_bits(&self) -> io::Result<serialport::StopBits>;
    fn timeout(&self) -> io::Result<std::time::Duration>;
    fn set_baud_rate(&mut self, baud_rate: u32) -> io::Result<()>;
    fn set_data_bits(&mut self, data_bits: serialport::DataBits) -> io::Result<()>;
    fn set_flow_control(&mut self, flow_control: serialport::FlowControl) -> io::Result<()>;
    fn set_parity(&mut self, parity: serialport::Parity) -> io::Result<()>;
    fn set_stop_bits(&mut self, stop_bits: serialport::StopBits) -> io::Result<()>;
    fn set_timeout(&mut self, timeout: std::time::Duration) -> io::Result<()>;
    fn write_request_to_send(&mut self, level: bool) -> io::Result<()>;
    fn write_data_terminal_ready(&mut self, level: bool) -> io::Result<()>;
    fn read_clear_to_send(&mut self) -> io::Result<bool>;
    fn read_data_set_ready(&mut self) -> io::Result<bool>;
    fn read_ring_indicator(&mut self) -> io::Result<bool>;
    fn read_carrier_detect(&mut self) -> io::Result<bool>;
    fn bytes_to_read(&self) -> io::Result<u32>;
    fn bytes_to_write(&self) -> io::Result<u32>;
    fn clear(&self, buffer_to_clear: serialport::ClearBuffer) -> io::Result<()>;
    fn try_clone(&self) -> io::Result<Box<dyn SerialPort>>;
    fn set_break(&self) -> io::Result<()>;
    fn clear_break(&self) -> io::Result<()>;
}

#[allow(dead_code)]
pub struct MockSerialPort {
    pub buffer: Vec<u8>,
    pub baud_rate: u32,
    pub data_bits: serialport::DataBits,
    pub flow_control: serialport::FlowControl,
    pub parity: serialport::Parity,
    pub stop_bits: serialport::StopBits,
    pub timeout: Duration,
}

#[allow(dead_code)]
impl MockSerialPort {
    pub fn new() -> Self {
        Self {
            buffer: Vec::new(),
            baud_rate: 9600,
            data_bits: serialport::DataBits::Eight,
            flow_control: serialport::FlowControl::None,
            parity: serialport::Parity::None,
            stop_bits: serialport::StopBits::One,
            timeout: Duration::from_millis(1000),
        }
    }
}

impl SerialPort for MockSerialPort {
    fn name(&self) -> Option<String> {
        Some("COM1".to_string())
    }

    fn baud_rate(&self) -> Result<u32, serialport::Error> {
        Ok(self.baud_rate)
    }

    fn data_bits(&self) -> Result<serialport::DataBits, serialport::Error> {
        Ok(self.data_bits)
    }

    fn flow_control(&self) -> Result<serialport::FlowControl, serialport::Error> {
        Ok(self.flow_control)
    }

    fn parity(&self) -> Result<serialport::Parity, serialport::Error> {
        Ok(self.parity)
    }

    fn stop_bits(&self) -> Result<serialport::StopBits, serialport::Error> {
        Ok(self.stop_bits)
    }

    fn timeout(&self) -> Duration {
        self.timeout
    }

    fn set_baud_rate(&mut self, rate: u32) -> Result<(), serialport::Error> {
        self.baud_rate = rate;
        Ok(())
    }

    fn set_data_bits(&mut self, bits: serialport::DataBits) -> Result<(), serialport::Error> {
        self.data_bits = bits;
        Ok(())
    }

    fn set_flow_control(&mut self, flow: serialport::FlowControl) -> Result<(), serialport::Error> {
        self.flow_control = flow;
        Ok(())
    }

    fn set_parity(&mut self, parity: serialport::Parity) -> Result<(), serialport::Error> {
        self.parity = parity;
        Ok(())
    }

    fn set_stop_bits(&mut self, bits: serialport::StopBits) -> Result<(), serialport::Error> {
        self.stop_bits = bits;
        Ok(())
    }

    fn set_timeout(&mut self, timeout: Duration) -> Result<(), serialport::Error> {
        self.timeout = timeout;
        Ok(())
    }

    fn write_request_to_send(&mut self, _level: bool) -> Result<(), serialport::Error> {
        Ok(())
    }

    fn write_data_terminal_ready(&mut self, _level: bool) -> Result<(), serialport::Error> {
        Ok(())
    }

    fn read_clear_to_send(&mut self) -> Result<bool, serialport::Error> {
        Ok(true)
    }

    fn read_data_set_ready(&mut self) -> Result<bool, serialport::Error> {
        Ok(true)
    }

    fn read_ring_indicator(&mut self) -> Result<bool, serialport::Error> {
        Ok(true)
    }

    fn read_carrier_detect(&mut self) -> Result<bool, serialport::Error> {
        Ok(true)
    }

    fn bytes_to_read(&self) -> Result<u32, serialport::Error> {
        Ok(self.buffer.len() as u32)
    }

    fn bytes_to_write(&self) -> Result<u32, serialport::Error> {
        Ok(0)
    }

    fn clear(&self, _buffer_to_clear: serialport::ClearBuffer) -> Result<(), serialport::Error> {
        Ok(())
    }

    fn try_clone(&self) -> Result<Box<dyn SerialPort>, serialport::Error> {
        Ok(Box::new(MockSerialPort {
            buffer: self.buffer.clone(),
            baud_rate: self.baud_rate,
            data_bits: self.data_bits,
            flow_control: self.flow_control,
            parity: self.parity,
            stop_bits: self.stop_bits,
            timeout: self.timeout,
        }))
    }

    fn set_break(&self) -> Result<(), serialport::Error> {
        Ok(())
    }

    fn clear_break(&self) -> Result<(), serialport::Error> {
        Ok(())
    }
}

impl Read for MockSerialPort {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let len = std::cmp::min(buf.len(), self.buffer.len());
        if len > 0 {
            buf[..len].copy_from_slice(&self.buffer[..len]);
            self.buffer.drain(..len);
        }
        Ok(len)
    }
}

impl Write for MockSerialPort {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.buffer.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[allow(dead_code)]
pub fn create_mock_serialport_info() -> SerialportInfo {
    SerialportInfo {
        serialport: Box::new(MockSerialPort::new()),
        sender: None,
        thread_handle: None,
    }
} 