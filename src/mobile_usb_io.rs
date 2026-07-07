//! Thin USB I/O via JNI → Kotlin `UsbNative` (no Tauri `@Command`).

use crate::error::Error;
#[cfg(target_os = "android")]
use crate::mobile_usb_jni;
use crate::state::{ClearBuffer, DataBits, FlowControl, Parity, StopBits};
use serde::Deserialize;
use std::collections::HashMap;
use std::time::Duration;

#[derive(Deserialize, Debug)]
struct PortInfo {
    #[serde(rename = "type")]
    type_: String,
    vid: String,
    pid: String,
    manufacturer: String,
    product: String,
    serial_number: String,
}

#[derive(Deserialize, Debug)]
struct AvailablePortsResponse {
    ports: HashMap<String, PortInfo>,
}

#[derive(Clone, Copy, Default)]
pub struct MobileUsbIo;

impl MobileUsbIo {
    pub fn new() -> Self {
        Self
    }

    fn ctl_ok(
        &self,
        path: &str,
        op: &str,
        baud_rate: u32,
        timeout_ms: u64,
        data_bits: u8,
        flow_control: u8,
        parity: u8,
        stop_bits: u8,
        buffer_type: i32,
        fallback: &str,
    ) -> Result<(), Error> {
        #[cfg(target_os = "android")]
        {
            if mobile_usb_jni::call_ctl(
                path,
                op,
                baud_rate,
                timeout_ms,
                data_bits,
                flow_control,
                parity,
                stop_bits,
                buffer_type,
            )? {
                return Ok(());
            }
            return Err(Error::new(fallback.to_string()));
        }
        #[cfg(not(target_os = "android"))]
        let _ = (
            path,
            op,
            baud_rate,
            timeout_ms,
            data_bits,
            flow_control,
            parity,
            stop_bits,
            buffer_type,
            fallback,
        );
        #[cfg(not(target_os = "android"))]
        Err(Error::new("USB I/O only on Android"))
    }

    pub fn available_ports(
        &self,
    ) -> Result<HashMap<String, HashMap<String, String>>, Error> {
        #[cfg(target_os = "android")]
        {
            let json = mobile_usb_jni::call_enumerate_json()?;
            let response: AvailablePortsResponse = serde_json::from_str(&json)
                .map_err(|e| Error::new(format!("Invalid enumerate JSON: {e}")))?;
            let mut result = HashMap::new();
            for (name, info) in response.ports {
                let mut map = HashMap::new();
                map.insert("type".to_string(), info.type_);
                map.insert("vid".to_string(), info.vid);
                map.insert("pid".to_string(), info.pid);
                map.insert("manufacturer".to_string(), info.manufacturer);
                map.insert("product".to_string(), info.product);
                map.insert("serial_number".to_string(), info.serial_number);
                result.insert(name, map);
            }
            return Ok(result);
        }
        #[cfg(not(target_os = "android"))]
        Ok(HashMap::new())
    }

    /// Rust tracks managed ports; Kotlin enumeration-only layer has no managed list.
    pub fn managed_ports(&self) -> Result<Vec<String>, Error> {
        Ok(Vec::new())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn open(
        &self,
        path: String,
        baud_rate: u32,
        data_bits: DataBits,
        flow_control: FlowControl,
        parity: Parity,
        stop_bits: StopBits,
        timeout: u64,
    ) -> Result<(), Error> {
        #[cfg(target_os = "android")]
        {
            return mobile_usb_jni::call_open(
                &path,
                baud_rate,
                data_bits.as_u8(),
                flow_control.as_u8(),
                parity.as_u8(),
                stop_bits.as_u8(),
                timeout,
            );
        }
        #[cfg(not(target_os = "android"))]
        let _ = (
            path,
            baud_rate,
            data_bits,
            flow_control,
            parity,
            stop_bits,
            timeout,
        );
        #[cfg(not(target_os = "android"))]
        Err(Error::new("USB I/O only on Android"))
    }

    pub fn close(&self, path: &str) -> Result<(), Error> {
        #[cfg(target_os = "android")]
        return mobile_usb_jni::call_close(Some(path));
        #[cfg(not(target_os = "android"))]
        {
            let _ = path;
            Err(Error::new("USB I/O only on Android"))
        }
    }

    pub fn close_all(&self) -> Result<(), Error> {
        #[cfg(target_os = "android")]
        return mobile_usb_jni::call_close(None);
        #[cfg(not(target_os = "android"))]
        Err(Error::new("USB I/O only on Android"))
    }

    pub fn write(&self, path: &str, data: &[u8]) -> Result<usize, Error> {
        #[cfg(target_os = "android")]
        return mobile_usb_jni::call_write(path, data);
        #[cfg(not(target_os = "android"))]
        {
            let _ = (path, data);
            Err(Error::new("USB I/O only on Android"))
        }
    }

    pub fn read_poll(
        &self,
        path: &str,
        timeout_ms: u64,
        size: usize,
    ) -> Result<Vec<u8>, Error> {
        #[cfg(target_os = "android")]
        return mobile_usb_jni::call_read(path, timeout_ms, size);
        #[cfg(not(target_os = "android"))]
        {
            let _ = (path, timeout_ms, size);
            Err(Error::new("USB I/O only on Android"))
        }
    }

    pub fn read_text(
        &self,
        path: &str,
        timeout_ms: u64,
        size: usize,
    ) -> Result<String, Error> {
        let bytes = self.read_poll(path, timeout_ms, size)?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    /// SIOM starts on `usbOpen`; no-op for Rust watch path.
    pub fn start_listen(&self, _path: &str) -> Result<(), Error> {
        Ok(())
    }

    /// SIOM stops on `usbClose`; no-op for Rust unwatch path.
    pub fn stop_listen(&self, _path: &str) -> Result<(), Error> {
        Ok(())
    }

    pub fn clear_buffer(&self, path: &str, buffer_type: ClearBuffer) -> Result<(), Error> {
        self.ctl_ok(
            path,
            "clearBuffer",
            0,
            0,
            0,
            0,
            0,
            0,
            match buffer_type {
                ClearBuffer::Input => 0,
                ClearBuffer::Output => 1,
                ClearBuffer::All => 2,
            },
            "Failed to clear buffer",
        )
    }

    pub fn set_baud_rate(&self, path: &str, baud_rate: u32) -> Result<(), Error> {
        self.ctl_ok(
            path,
            "setBaudRate",
            baud_rate,
            0,
            0,
            0,
            0,
            0,
            0,
            "Failed to set baud rate",
        )
    }

    pub fn set_timeout(&self, path: &str, timeout: Duration) -> Result<(), Error> {
        self.ctl_ok(
            path,
            "setTimeout",
            0,
            timeout.as_millis() as u64,
            0,
            0,
            0,
            0,
            0,
            "Failed to set timeout",
        )
    }

    pub fn set_data_bits(&self, path: &str, data_bits: DataBits) -> Result<(), Error> {
        self.ctl_ok(
            path,
            "setDataBits",
            0,
            0,
            data_bits.as_u8(),
            0,
            0,
            0,
            0,
            "Failed to set data bits",
        )
    }

    pub fn set_flow_control(&self, path: &str, flow_control: FlowControl) -> Result<(), Error> {
        self.ctl_ok(
            path,
            "setFlowControl",
            0,
            0,
            0,
            flow_control.as_u8(),
            0,
            0,
            0,
            "Failed to set flow control",
        )
    }

    pub fn set_parity(&self, path: &str, parity: Parity) -> Result<(), Error> {
        self.ctl_ok(
            path,
            "setParity",
            0,
            0,
            0,
            0,
            parity.as_u8(),
            0,
            0,
            "Failed to set parity",
        )
    }

    pub fn set_stop_bits(&self, path: &str, stop_bits: StopBits) -> Result<(), Error> {
        self.ctl_ok(
            path,
            "setStopBits",
            0,
            0,
            0,
            0,
            0,
            stop_bits.as_u8(),
            0,
            "Failed to set stop bits",
        )
    }

    pub fn write_rts(&self, path: &str, level: bool) -> Result<(), Error> {
        if mobile_signal(path, "writeRts", level)? {
            Ok(())
        } else {
            Err(Error::new("Failed to set RTS"))
        }
    }

    pub fn write_dtr(&self, path: &str, level: bool) -> Result<(), Error> {
        if mobile_signal(path, "writeDtr", level)? {
            Ok(())
        } else {
            Err(Error::new("Failed to set DTR"))
        }
    }

    pub fn read_cts(&self, path: &str) -> Result<bool, Error> {
        mobile_signal(path, "readCts", false)
    }

    pub fn read_dsr(&self, path: &str) -> Result<bool, Error> {
        mobile_signal(path, "readDsr", false)
    }

    pub fn read_ri(&self, path: &str) -> Result<bool, Error> {
        mobile_signal(path, "readRi", false)
    }

    pub fn read_cd(&self, path: &str) -> Result<bool, Error> {
        mobile_signal(path, "readCd", false)
    }

    pub fn bytes_to_write(&self, path: &str) -> Result<u32, Error> {
        #[cfg(target_os = "android")]
        return mobile_usb_jni::call_ctl_bytes_to_write(path);
        #[cfg(not(target_os = "android"))]
        {
            let _ = path;
            Err(Error::new("USB I/O only on Android"))
        }
    }

    pub fn set_break(&self, path: &str) -> Result<(), Error> {
        self.ctl_ok(
            path, "setBreak", 0, 0, 0, 0, 0, 0, 0, "Failed to set break",
        )
    }

    pub fn clear_break(&self, path: &str) -> Result<(), Error> {
        self.ctl_ok(
            path,
            "clearBreak",
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            "Failed to clear break",
        )
    }

    pub fn cancel_read(&self, path: &str) -> Result<(), Error> {
        self.ctl_ok(
            path,
            "cancelRead",
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            "Failed to cancel read",
        )
    }
}

#[cfg(target_os = "android")]
fn mobile_signal(path: &str, op: &str, level: bool) -> Result<bool, Error> {
    mobile_usb_jni::call_signal(path, op, level)
}

#[cfg(not(target_os = "android"))]
fn mobile_signal(path: &str, op: &str, level: bool) -> Result<bool, Error> {
    let _ = (path, op, level);
    Err(Error::new("USB I/O only on Android"))
}
