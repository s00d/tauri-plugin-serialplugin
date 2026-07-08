//! Expand Kotlin USB device list into session paths via [`ProbeTable`] (no fd / no I/O).

use crate::android::usb_path;
use crate::error::Error;
use android_usb_serial::probe::ProbeTable;
use android_usb_serial::transport::InterfaceInfo;
use std::collections::HashMap;

#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct IfaceJson {
    id: u8,
    class: u8,
    subclass: u8,
    protocol: u8,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct DeviceEntry {
    #[serde(rename = "type", default)]
    pub type_: String,
    #[serde(default)]
    pub vid: String,
    #[serde(default)]
    pub pid: String,
    #[serde(default)]
    pub manufacturer: String,
    #[serde(default)]
    pub product: String,
    #[serde(default)]
    pub serial_number: String,
    #[serde(default)]
    pub interfaces: Vec<IfaceJson>,
}

fn parse_vid_pid(hex: &str) -> Result<u16, Error> {
    let s = hex
        .strip_prefix("0x")
        .or_else(|| hex.strip_prefix("0X"))
        .unwrap_or(hex);
    u16::from_str_radix(s, 16).map_err(|e| Error::new(format!("invalid vid/pid {hex}: {e}")))
}

fn to_iface(info: &IfaceJson) -> InterfaceInfo {
    InterfaceInfo {
        id: info.id,
        class: info.class,
        subclass: info.subclass,
        protocol: info.protocol,
    }
}

fn meta_map(entry: &DeviceEntry) -> HashMap<String, String> {
    let mut map = HashMap::new();
    map.insert("type".to_string(), entry.type_.clone());
    map.insert("vid".to_string(), entry.vid.clone());
    map.insert("pid".to_string(), entry.pid.clone());
    map.insert("manufacturer".to_string(), entry.manufacturer.clone());
    map.insert("product".to_string(), entry.product.clone());
    map.insert("serial_number".to_string(), entry.serial_number.clone());
    map
}

/// One Kotlin `deviceName` → one or more session paths (`device` or `device#N`).
pub(crate) fn expand_devices(
    devices: HashMap<String, DeviceEntry>,
    single_port_per_device: bool,
) -> Result<HashMap<String, HashMap<String, String>>, Error> {
    let table = ProbeTable::default_table();
    let mut result = HashMap::new();

    for (device_name, entry) in devices {
        let vid = parse_vid_pid(&entry.vid)?;
        let pid = parse_vid_pid(&entry.pid)?;
        let ifaces: Vec<InterfaceInfo> = entry.interfaces.iter().map(to_iface).collect();
        let driver = table.find(vid, pid, &ifaces);
        let port_count = table.port_count_product(vid, pid, driver, &ifaces);
        if port_count == 0 {
            continue;
        }
        let base = meta_map(&entry);
        for port_index in 0..port_count {
            let path = usb_path::session_key(&device_name, port_index, port_count);
            result.insert(path, base.clone());
        }
    }

    Ok(crate::port::list::apply_android_single_port_per_device(
        result,
        single_port_per_device,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ftdi_dual() -> DeviceEntry {
        DeviceEntry {
            type_: "Usb".into(),
            vid: "0x0403".into(),
            pid: "0x6010".into(),
            manufacturer: String::new(),
            product: "FT2232".into(),
            serial_number: String::new(),
            interfaces: vec![
                IfaceJson {
                    id: 0,
                    class: 255,
                    subclass: 255,
                    protocol: 255,
                },
                IfaceJson {
                    id: 1,
                    class: 255,
                    subclass: 255,
                    protocol: 255,
                },
            ],
        }
    }

    #[test]
    fn expands_ft2232_to_two_paths() {
        let devices = HashMap::from([("/dev/bus/usb/001/002".to_string(), ftdi_dual())]);
        let out = expand_devices(devices, false).unwrap();
        assert_eq!(out.len(), 2);
        assert!(out.contains_key("/dev/bus/usb/001/002#0"));
        assert!(out.contains_key("/dev/bus/usb/001/002#1"));
    }

    #[test]
    fn ch340_single_path() {
        let devices = HashMap::from([(
            "/dev/bus/usb/001/002".to_string(),
            DeviceEntry {
                type_: "Usb".into(),
                vid: "0x1A86".into(),
                pid: "0x7523".into(),
                manufacturer: String::new(),
                product: "USB Serial".into(),
                serial_number: String::new(),
                interfaces: vec![IfaceJson {
                    id: 0,
                    class: 255,
                    subclass: 0,
                    protocol: 0,
                }],
            },
        )]);
        let out = expand_devices(devices, true).unwrap();
        assert_eq!(out.len(), 1);
        assert!(out.contains_key("/dev/bus/usb/001/002"));
    }
}
