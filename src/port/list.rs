//! Helpers for normalizing port lists returned by `serialport-rs`.

#[cfg(not(target_os = "android"))]
use crate::error::Error;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use crate::state::{BLUETOOTH, PCI, UNKNOWN, USB};
use std::collections::HashMap;

#[cfg(not(any(target_os = "android", target_os = "ios")))]
use serialport::SerialPortType;

/// Build metadata map for one enumerated port (desktop).
#[cfg(not(any(target_os = "android", target_os = "ios")))]
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

/// iOS has no local serial ports (only `tcp://` works there): an empty list.
#[cfg(target_os = "ios")]
pub fn enumerate_available_ports(
    _single_port_per_device: bool,
) -> Result<HashMap<String, HashMap<String, String>>, Error> {
    Ok(HashMap::new())
}

/// Enumerate available serial ports (desktop), with optional macOS de-duplication.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
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
    // serialport truncates USB serial numbers at '-' (serialport-rs#351). The full
    // one is the device-instance key name; its "Device Parameters\PortName" maps it
    // to COMx. Read the registry directly: `wmic` is gone from Windows 11 24H2+ and
    // spawning it flashed a console window on every port-list poll.
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;

    let Ok(usb) =
        RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(r"SYSTEM\CurrentControlSet\Enum\USB")
    else {
        return;
    };
    for vid_pid in usb.enum_keys().flatten() {
        let Ok(device) = usb.open_subkey(&vid_pid) else {
            continue;
        };
        for instance in device.enum_keys().flatten() {
            let Ok(params) = device.open_subkey(format!(r"{instance}\Device Parameters")) else {
                continue;
            };
            let Ok(port_name) = params.get_value::<String, _>("PortName") else {
                continue;
            };
            if let Some(info) = ports.get_mut(&port_name) {
                let current = info.get("serial_number").map(String::as_str).unwrap_or("");
                // Only extend a truncated serial; composite-device instance ids
                // ("6&2f3c...&0&0000") are not serial numbers.
                if instance.len() > current.len()
                    && instance.starts_with(current)
                    && !current.is_empty()
                {
                    info.insert("serial_number".to_string(), instance.clone());
                }
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

/// When `single_port_per_device` is true, keeps one USB serial port per Android device
/// (lowest port index; paths without `#` are port 0).
pub fn apply_android_single_port_per_device(
    ports: HashMap<String, HashMap<String, String>>,
    single_port_per_device: bool,
) -> HashMap<String, HashMap<String, String>> {
    if !single_port_per_device {
        return ports;
    }
    let mut best: HashMap<String, (u32, String, HashMap<String, String>)> = HashMap::new();
    for (path, info) in ports {
        let (device, port_idx) = android_path_parts(&path);
        let replace = match best.get(device) {
            None => true,
            Some((idx, _, _)) => port_idx < *idx,
        };
        if replace {
            best.insert(device.to_string(), (port_idx, path, info));
        }
    }
    best.into_values()
        .map(|(_, path, info)| (path, info))
        .collect()
}

fn android_path_parts(path: &str) -> (&str, u32) {
    if let Some(idx) = path.rfind('#') {
        let port = path[idx + 1..].parse().unwrap_or(0);
        (&path[..idx], port)
    } else {
        (path, 0)
    }
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

    #[test]
    fn android_keeps_lowest_port_index_per_device() {
        let ports = HashMap::from([
            ("/dev/bus/usb/001/002#1".to_string(), info("USB")),
            ("/dev/bus/usb/001/002#0".to_string(), info("USB")),
            ("/dev/bus/usb/001/003".to_string(), info("USB")),
        ]);
        let out = apply_android_single_port_per_device(ports, true);
        assert_eq!(out.len(), 2);
        assert!(out.contains_key("/dev/bus/usb/001/002#0"));
        assert!(out.contains_key("/dev/bus/usb/001/003"));
    }
}
