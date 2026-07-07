//! Helpers for normalizing port lists returned by `serialport-rs`.

#[cfg(desktop)]
use crate::error::Error;
#[cfg(desktop)]
use crate::state::{BLUETOOTH, PCI, UNKNOWN, USB};
use std::collections::HashMap;

#[cfg(desktop)]
use serialport::SerialPortType;

/// Build metadata map for one enumerated port (desktop).
#[cfg(desktop)]
pub fn port_info_from_type(port: SerialPortType) -> HashMap<String, String> {
    let mut port_info: HashMap<String, String> = HashMap::new();
    port_info.insert("type".to_string(), UNKNOWN.to_string());
    port_info.insert("vid".to_string(), UNKNOWN.to_string());
    port_info.insert("pid".to_string(), UNKNOWN.to_string());
    port_info.insert("serial_number".to_string(), UNKNOWN.to_string());
    port_info.insert("manufacturer".to_string(), UNKNOWN.to_string());
    port_info.insert("product".to_string(), UNKNOWN.to_string());

    match port {
        SerialPortType::UsbPort(info) => {
            port_info.insert("type".to_string(), USB.to_string());
            port_info.insert("vid".to_string(), info.vid.to_string());
            port_info.insert("pid".to_string(), info.pid.to_string());
            port_info.insert(
                "serial_number".to_string(),
                info.serial_number.unwrap_or_else(|| UNKNOWN.to_string()),
            );
            port_info.insert(
                "manufacturer".to_string(),
                info.manufacturer.unwrap_or_else(|| UNKNOWN.to_string()),
            );
            port_info.insert(
                "product".to_string(),
                info.product.unwrap_or_else(|| UNKNOWN.to_string()),
            );
        }
        SerialPortType::BluetoothPort => {
            port_info.insert("type".to_string(), BLUETOOTH.to_string());
        }
        SerialPortType::PciPort => {
            port_info.insert("type".to_string(), PCI.to_string());
        }
        SerialPortType::Unknown => {
            port_info.insert("type".to_string(), UNKNOWN.to_string());
        }
    }

    port_info
}

/// Enumerate available serial ports (desktop), with optional macOS de-duplication.
#[cfg(desktop)]
pub fn enumerate_available_ports(
    single_port_per_device: bool,
) -> Result<HashMap<String, HashMap<String, String>>, Error> {
    let mut list = serialport::available_ports()
        .map_err(|e| Error::String(format!("Failed to enumerate serial ports: {}", e)))?;
    list.sort_by(|a, b| a.port_name.cmp(&b.port_name));

    let mut result_list: HashMap<String, HashMap<String, String>> = HashMap::new();
    for p in list {
        result_list.insert(p.port_name, port_info_from_type(p.port_type));
    }

    #[cfg(target_os = "windows")]
    enrich_windows_serial_numbers(&mut result_list);

    Ok(apply_single_port_per_device(
        result_list,
        single_port_per_device,
    ))
}

#[cfg(target_os = "windows")]
fn enrich_windows_serial_numbers(ports: &mut HashMap<String, HashMap<String, String>>) {
    use std::process::Command;

    let output = match Command::new("wmic")
        .args([
            "path",
            "Win32_SerialPort",
            "get",
            "DeviceID,PNPDeviceID",
            "/format:csv",
        ])
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => return,
    };

    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        if line.is_empty() || line.starts_with("Node,") {
            continue;
        }
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() < 3 {
            continue;
        }
        let device_id = parts[1].trim();
        let pnp = parts[2].trim();
        if device_id.is_empty() || pnp.is_empty() {
            continue;
        }
        let Some(pnp_serial) = pnp.rsplit('\\').next() else {
            continue;
        };
        if pnp_serial.is_empty() {
            continue;
        }
        if let Some(info) = ports.get_mut(device_id) {
            let current = info.get("serial_number").map(String::as_str).unwrap_or("");
            if pnp_serial.len() > current.len() {
                info.insert("serial_number".to_string(), pnp_serial.to_string());
            }
        }
    }
}

/// macOS exposes each serial device twice: `/dev/cu.*` (callout) and `/dev/tty.*` (dial-in).
/// `serialport-rs` returns both; Node.js `SerialPort.list()` keeps callout only.
///
/// When `single_port_per_device` is true, keeps one path per device suffix, preferring callout.
pub fn apply_single_port_per_device(
    ports: HashMap<String, HashMap<String, String>>,
    single_port_per_device: bool,
) -> HashMap<String, HashMap<String, String>> {
    if !single_port_per_device {
        return ports;
    }
    filter_macos_single_port_per_device(ports)
}

fn filter_macos_single_port_per_device(
    ports: HashMap<String, HashMap<String, String>>,
) -> HashMap<String, HashMap<String, String>> {
    let mut callout: HashMap<String, (String, HashMap<String, String>)> = HashMap::new();
    let mut dialin: HashMap<String, (String, HashMap<String, String>)> = HashMap::new();
    let mut other: HashMap<String, HashMap<String, String>> = HashMap::new();

    for (path, info) in ports {
        if let Some(suffix) = path.strip_prefix("/dev/cu.") {
            callout.insert(suffix.to_string(), (path, info));
        } else if let Some(suffix) = path.strip_prefix("/dev/tty.") {
            dialin.insert(suffix.to_string(), (path, info));
        } else {
            other.insert(path, info);
        }
    }

    let mut result = other;
    for (suffix, entry) in dialin {
        callout.entry(suffix).or_insert(entry);
    }
    for (_, (path, info)) in callout {
        result.insert(path, info);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(type_: &str) -> HashMap<String, String> {
        HashMap::from([("type".to_string(), type_.to_string())])
    }

    #[test]
    fn no_op_when_disabled() {
        let ports = HashMap::from([
            ("/dev/cu.usbmodem1".to_string(), info("USB")),
            ("/dev/tty.usbmodem1".to_string(), info("USB")),
        ]);
        let out = apply_single_port_per_device(ports.clone(), false);
        assert_eq!(out.len(), 2);
        assert_eq!(out, ports);
    }

    #[test]
    fn prefers_callout_over_dialin_for_same_suffix() {
        let ports = HashMap::from([
            ("/dev/tty.usbmodem1421".to_string(), info("USB")),
            ("/dev/cu.usbmodem1421".to_string(), info("USB")),
            (
                "/dev/cu.Bluetooth-Incoming-Port".to_string(),
                info("Bluetooth"),
            ),
            (
                "/dev/tty.Bluetooth-Incoming-Port".to_string(),
                info("Bluetooth"),
            ),
        ]);
        let out = apply_single_port_per_device(ports, true);
        assert_eq!(out.len(), 2);
        assert!(out.contains_key("/dev/cu.usbmodem1421"));
        assert!(out.contains_key("/dev/cu.Bluetooth-Incoming-Port"));
        assert!(!out.contains_key("/dev/tty.usbmodem1421"));
    }

    #[test]
    fn keeps_dialin_when_callout_missing() {
        let ports = HashMap::from([("/dev/tty.usbserial-ABC".to_string(), info("USB"))]);
        let out = apply_single_port_per_device(ports, true);
        assert_eq!(out.len(), 1);
        assert!(out.contains_key("/dev/tty.usbserial-ABC"));
    }

    #[test]
    fn preserves_non_macos_style_paths() {
        let ports = HashMap::from([
            ("COM3".to_string(), info("USB")),
            ("/dev/ttyUSB0".to_string(), info("USB")),
        ]);
        let out = apply_single_port_per_device(ports.clone(), true);
        assert_eq!(out, ports);
    }
}
