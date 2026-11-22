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

/// Enumerate available serial ports (desktop), with optional macOS de-duplication.
/// iOS has no local serial ports (only `tcp://` works there): an empty list.
#[cfg(not(target_os = "android"))]
pub fn enumerate_available_ports(
    single_port_per_device: bool,
) -> Result<HashMap<String, HashMap<String, String>>, Error> {
    #[cfg(target_os = "ios")]
    {
        let _ = single_port_per_device;
        Ok(HashMap::new())
    }
    #[cfg(not(target_os = "ios"))]
    {
        Ok(enumerate_with_locations(single_port_per_device)?.0)
    }
}

/// path -> 4.x info map.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
type PortListMap = HashMap<String, HashMap<String, String>>;

/// Raw `serialport` enumeration, sorted by path.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub(crate) fn raw_ports() -> Result<Vec<serialport::SerialPortInfo>, Error> {
    let mut list = serialport::available_ports()
        .map_err(|e| Error::String(format!("Failed to enumerate serial ports: {}", e)))?;
    list.sort_by(|a, b| a.port_name.cmp(&b.port_name));
    Ok(list)
}

/// [`enumerate_available_ports`] plus each USB port's location (one scan), so the
/// port-list monitor can see a different device re-plugged on the same path.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub(crate) fn enumerate_with_locations(
    single_port_per_device: bool,
) -> Result<(PortListMap, HashMap<String, String>), Error> {
    let list = raw_ports()?;
    let mut locations = HashMap::new();
    let mut result_list: HashMap<String, HashMap<String, String>> = HashMap::new();
    for p in list {
        if let Some(loc) = crate::port::details::usb_location(&p.port_type) {
            locations.insert(p.port_name.clone(), loc);
        }
        result_list.insert(p.port_name, port_info_from_type(p.port_type));
    }

    #[cfg(target_os = "windows")]
    for (port, instances) in windows_full_serials() {
        if let Some(info) = result_list.get_mut(&port) {
            let current = info.get("serial_number").map(String::as_str).unwrap_or("");
            if let Some(full) = pick_full_serial(current, &instances) {
                let full = full.to_string();
                info.insert("serial_number".to_string(), full);
            }
        }
    }

    Ok((
        apply_single_port_per_device(result_list, single_port_per_device),
        locations,
    ))
}

/// True when `full` (a USB device-instance key name) is the untruncated `current`
/// serial. Composite-device instance ids ("6&2f3c...&0&0000") are not serial numbers.
#[cfg(any(target_os = "windows", test))]
pub(crate) fn extends_serial(current: &str, full: &str) -> bool {
    full.len() > current.len() && full.starts_with(current) && !current.is_empty()
}

/// The first device instance that is the untruncated `current` serial (several
/// instances, a phantom one included, can carry one COM name).
#[cfg(any(target_os = "windows", test))]
pub(crate) fn pick_full_serial<'a>(current: &str, instances: &'a [String]) -> Option<&'a str> {
    instances
        .iter()
        .map(String::as_str)
        .find(|full| extends_serial(current, full))
}

/// COM port -> every USB device-instance key name with that PortName (one of them
/// is the full serial number).
#[cfg(target_os = "windows")]
pub(crate) fn windows_full_serials() -> HashMap<String, Vec<String>> {
    // serialport truncates USB serial numbers at '-' (serialport-rs#351). The full
    // one is the device-instance key name; its "Device Parameters\PortName" maps it
    // to COMx. Read the registry directly: `wmic` is gone from Windows 11 24H2+ and
    // spawning it flashed a console window on every port-list poll.
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;

    let mut out = HashMap::new();
    let Ok(usb) =
        RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(r"SYSTEM\CurrentControlSet\Enum\USB")
    else {
        return out;
    };
    for vid_pid in usb.enum_keys().flatten() {
        let Ok(device) = usb.open_subkey(&vid_pid) else {
            continue;
        };
        for instance in device.enum_keys().flatten() {
            let Ok(params) = device.open_subkey(format!(r"{instance}\Device Parameters")) else {
                continue;
            };
            if let Ok(port_name) = params.get_value::<String, _>("PortName") {
                out.entry(port_name).or_insert_with(Vec::new).push(instance);
            }
        }
    }
    out
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
    //! In `src/`: the Windows registry helpers are private (and pure), and the locations are
    //! only read by the port-list monitor.
    use super::*;

    /// Several device instances can map to one COM name (a phantom one included): the one
    /// that extends the truncated serial wins, as in 4.x; an unrelated or equal one is none.
    #[test]
    fn a_truncated_serial_is_completed_from_its_instances() {
        for (current, full, extends) in [
            ("A1B2", "A1B2-C3", true),
            ("", "6&2f3c&0&0000", false),
            ("A1B2", "6&2f3c&0&0000", false),
            ("A1B2", "A1B2", false),
        ] {
            assert_eq!(extends_serial(current, full), extends, "{current} {full}");
        }
        let instances = ["6&2f3c&0&0000", "A1B2-C3", "ZZZ"].map(String::from);
        assert_eq!(pick_full_serial("A1B2", &instances), Some("A1B2-C3"));
        assert_eq!(pick_full_serial("Q9", &instances), None);
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    #[test]
    fn locations_belong_to_listed_ports() {
        let (map, locations) = enumerate_with_locations(false).unwrap();
        assert!(locations.keys().all(|k| map.contains_key(k)));
        #[cfg(not(feature = "usb-port-details"))]
        assert!(locations.is_empty());
    }
}
