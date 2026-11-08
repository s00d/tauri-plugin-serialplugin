//! Typed per-port details ([`available_ports_detailed`]): numeric vid/pid, USB
//! interface number, physical location and Linux `/dev/serial` links. The 4.x string
//! map of [`SerialManager::available_ports`](crate::SerialManager::available_ports)
//! is unchanged.

use serde::{Deserialize, Serialize};
#[cfg(any(all(target_os = "linux", not(target_os = "android")), all(test, unix)))]
use std::collections::HashMap;
#[cfg(any(all(target_os = "linux", not(target_os = "android")), all(test, unix)))]
use std::path::{Path, PathBuf};

/// Kind of port, as in the 4.x `type` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PortType {
    #[serde(rename = "USB")]
    Usb,
    #[serde(rename = "Bluetooth")]
    Bluetooth,
    #[serde(rename = "PCI")]
    Pci,
    #[default]
    Unknown,
}

/// One enumerated port. Unknown values are `None` (not `"Unknown"`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct PortDetails {
    /// Port path (`/dev/ttyUSB0`, `/dev/cu.usbmodem1`, `COM3`, Android device path).
    pub path: String,
    pub port_type: PortType,
    pub vid: Option<u16>,
    pub pid: Option<u16>,
    /// Full serial number (on Windows untruncated, like the 4.x map).
    pub serial_number: Option<String>,
    pub manufacturer: Option<String>,
    pub product: Option<String>,
    /// USB interface number: tells the ports of one composite device (a modem's
    /// AT / NMEA / diag ports) apart. On macOS it can be the data interface. Desktop
    /// with the core feature `usb-port-details` only, else `None`.
    pub interface: Option<u8>,
    /// Physical USB location as `bus-port.port…` (Linux sysfs `busnum`/`devpath`,
    /// macOS IOKit `locationID`, Windows `LocationPaths`). Same socket, same value.
    /// Desktop with the core feature `usb-port-details` only, else `None`.
    pub location: Option<String>,
    /// Linux `/dev/serial/by-id/*` links to this port (empty elsewhere).
    pub by_id: Vec<String>,
    /// Linux `/dev/serial/by-path/*` links to this port (empty elsewhere).
    pub by_path: Vec<String>,
}

/// USB location string of a `serialport` port type, if it has one (feature
/// `usb-port-details`; `None` without it).
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub(crate) fn usb_location(port: &serialport::SerialPortType) -> Option<String> {
    #[cfg(feature = "usb-port-details")]
    if let serialport::SerialPortType::UsbPort(info) = port {
        return info.location.as_ref().map(|l| l.to_string());
    }
    let _ = port;
    None
}

/// Convert one `serialport` entry (desktop).
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn details_from_type(path: String, port: serialport::SerialPortType) -> PortDetails {
    use serialport::SerialPortType as T;
    let location = usb_location(&port);
    match port {
        T::UsbPort(info) => PortDetails {
            path,
            port_type: PortType::Usb,
            vid: Some(info.vid),
            pid: Some(info.pid),
            serial_number: info.serial_number,
            manufacturer: info.manufacturer,
            product: info.product,
            #[cfg(feature = "usb-port-details")]
            interface: info.interface,
            location,
            ..Default::default()
        },
        T::BluetoothPort => PortDetails {
            path,
            port_type: PortType::Bluetooth,
            ..Default::default()
        },
        T::PciPort => PortDetails {
            path,
            port_type: PortType::Pci,
            ..Default::default()
        },
        T::Unknown => PortDetails {
            path,
            ..Default::default()
        },
    }
}

/// Convert a 4.x string map (Android, iOS): `"Unknown"` -> `None`, vid/pid hex
/// (`0x1A86`) or decimal. No interface / location there.
pub fn details_from_info_map(
    path: String,
    info: &std::collections::HashMap<String, String>,
) -> PortDetails {
    let text = |k: &str| {
        info.get(k)
            .filter(|v| v.as_str() != crate::state::UNKNOWN)
            .cloned()
    };
    let id = |k: &str| {
        let v = text(k)?;
        match v.strip_prefix("0x").or_else(|| v.strip_prefix("0X")) {
            Some(hex) => u16::from_str_radix(hex, 16).ok(),
            None => v.parse().ok(),
        }
    };
    let port_type = match info.get("type").map(String::as_str) {
        Some(crate::state::USB) => PortType::Usb,
        Some(crate::state::BLUETOOTH) => PortType::Bluetooth,
        Some(crate::state::PCI) => PortType::Pci,
        _ => PortType::Unknown,
    };
    PortDetails {
        path,
        port_type,
        vid: id("vid"),
        pid: id("pid"),
        serial_number: text("serial_number"),
        manufacturer: text("manufacturer"),
        product: text("product"),
        ..Default::default()
    }
}

/// Symlinks in `dir`, grouped by their canonical target (sorted link paths).
/// Dangling links and a missing `dir` are skipped.
#[cfg(any(all(target_os = "linux", not(target_os = "android")), all(test, unix)))]
fn serial_links(dir: &Path) -> HashMap<PathBuf, Vec<String>> {
    let mut out: HashMap<PathBuf, Vec<String>> = HashMap::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let link = entry.path();
        if let Ok(target) = link.canonicalize() {
            out.entry(target)
                .or_default()
                .push(link.display().to_string());
        }
    }
    out.values_mut().for_each(|v| v.sort());
    out
}

/// Every serial port with typed details, sorted by path (both macOS `cu.` and
/// `tty.` nodes, no de-duplication). iOS: empty.
#[cfg(not(target_os = "android"))]
pub fn available_ports_detailed() -> Result<Vec<PortDetails>, crate::error::Error> {
    #[cfg(target_os = "ios")]
    {
        Ok(Vec::new())
    }
    #[cfg(not(target_os = "ios"))]
    {
        #[allow(unused_mut)]
        let mut out: Vec<PortDetails> = crate::port::list::raw_ports()?
            .into_iter()
            .map(|p| details_from_type(p.port_name, p.port_type))
            .collect();

        #[cfg(target_os = "windows")]
        {
            let full = crate::port::list::windows_full_serials();
            for d in &mut out {
                let picked = match (d.serial_number.as_deref(), full.get(&d.path)) {
                    (Some(current), Some(instances)) => {
                        crate::port::list::pick_full_serial(current, instances)
                    }
                    _ => None,
                };
                if let Some(f) = picked.map(str::to_string) {
                    d.serial_number = Some(f);
                }
            }
        }

        #[cfg(target_os = "linux")]
        {
            let by_id = serial_links(Path::new("/dev/serial/by-id"));
            let by_path = serial_links(Path::new("/dev/serial/by-path"));
            for d in &mut out {
                if let Ok(target) = Path::new(&d.path).canonicalize() {
                    d.by_id = by_id.get(&target).cloned().unwrap_or_default();
                    d.by_path = by_path.get(&target).cloned().unwrap_or_default();
                }
            }
        }

        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[cfg(all(
        feature = "usb-port-details",
        not(any(target_os = "android", target_os = "ios"))
    ))]
    fn quectel(interface: u8) -> serialport::SerialPortType {
        serialport::SerialPortType::UsbPort(serialport::UsbPortInfo {
            vid: 0x2c7c,
            pid: 0x0125,
            serial_number: Some("a1b2c3".into()),
            manufacturer: Some("Quectel".into()),
            product: Some("EG25-G".into()),
            location: Some(serialport::Location::new("1".into(), vec![2, 3])),
            interface: Some(interface),
        })
    }

    /// The 4 ports of one Quectel modem look the same in the 4.x map; the interface
    /// number tells them apart.
    #[cfg(all(
        feature = "usb-port-details",
        not(any(target_os = "android", target_os = "ios"))
    ))]
    #[test]
    fn quectel_ports_differ_by_interface_number() {
        let ports: Vec<PortDetails> = (0..4u8)
            .map(|i| details_from_type(format!("/dev/ttyUSB{i}"), quectel(i)))
            .collect();
        for (i, d) in ports.iter().enumerate() {
            assert_eq!(d.port_type, PortType::Usb);
            assert_eq!(d.vid, Some(0x2c7c));
            assert_eq!(d.pid, Some(0x0125));
            assert_eq!(d.serial_number.as_deref(), Some("a1b2c3"));
            assert_eq!(d.manufacturer.as_deref(), Some("Quectel"));
            assert_eq!(d.product.as_deref(), Some("EG25-G"));
            assert_eq!(d.location.as_deref(), Some("1-2.3"));
            assert_eq!(d.interface, Some(i as u8));
        }
        let mut ifaces: Vec<_> = ports.iter().map(|d| d.interface).collect();
        ifaces.dedup();
        assert_eq!(ifaces.len(), 4);

        let maps: Vec<_> = (0..4u8)
            .map(|i| crate::port::list::port_info_from_type(quectel(i)))
            .collect();
        assert!(
            maps.iter().all(|m| m == &maps[0]),
            "4.x map cannot tell them apart"
        );
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    #[test]
    fn non_usb_ports_have_no_usb_fields() {
        let d = details_from_type("/dev/ttyS0".into(), serialport::SerialPortType::PciPort);
        assert_eq!(d.port_type, PortType::Pci);
        assert_eq!(d.vid, None);
        assert_eq!(d.interface, None);
        assert_eq!(d.location, None);
        let bt = details_from_type("x".into(), serialport::SerialPortType::BluetoothPort);
        assert_eq!(bt.port_type, PortType::Bluetooth);
    }

    /// Android and iOS go through the 4.x map: hex ("0x1A86") or decimal vid/pid,
    /// "Unknown" becomes `None`.
    #[test]
    fn from_info_map_parses_vid_pid_and_unknown() {
        let map = HashMap::from([
            ("type".to_string(), "USB".to_string()),
            ("vid".to_string(), "0x1A86".to_string()),
            ("pid".to_string(), "29987".to_string()),
            ("serial_number".to_string(), "Unknown".to_string()),
            ("manufacturer".to_string(), "QinHeng".to_string()),
            ("product".to_string(), "Unknown".to_string()),
        ]);
        let d = details_from_info_map("/dev/bus/usb/001/002#1".into(), &map);
        assert_eq!(d.port_type, PortType::Usb);
        assert_eq!(d.vid, Some(0x1a86));
        assert_eq!(d.pid, Some(29987));
        assert_eq!(d.serial_number, None);
        assert_eq!(d.manufacturer.as_deref(), Some("QinHeng"));
        assert_eq!(d.product, None);
        let empty = details_from_info_map("x".into(), &HashMap::new());
        assert_eq!(empty.port_type, PortType::Unknown);
    }

    #[cfg(unix)]
    #[test]
    fn serial_links_maps_targets_to_link_names() {
        let root = std::env::temp_dir().join(format!("sp-links-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let by_id = root.join("by-id");
        std::fs::create_dir_all(&by_id).unwrap();
        let dev = root.join("ttyUSB0");
        std::fs::write(&dev, b"").unwrap();
        std::os::unix::fs::symlink(&dev, by_id.join("usb-Quectel_EG25-if00-port0")).unwrap();
        std::os::unix::fs::symlink("../ttyUSB0", by_id.join("usb-alias")).unwrap();
        std::os::unix::fs::symlink(root.join("gone"), by_id.join("dangling")).unwrap();

        let links = serial_links(&by_id);
        let target = dev.canonicalize().unwrap();
        assert_eq!(
            links.get(&target).cloned().unwrap_or_default(),
            vec![
                by_id
                    .join("usb-Quectel_EG25-if00-port0")
                    .display()
                    .to_string(),
                by_id.join("usb-alias").display().to_string(),
            ]
        );
        assert_eq!(links.len(), 1);
        assert!(serial_links(&root.join("missing")).is_empty());
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// Live check: `cargo test -p serialplugin-core live_print_details -- --ignored --nocapture`.
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    #[test]
    #[ignore = "needs real serial hardware"]
    fn live_print_details() {
        for d in available_ports_detailed().unwrap() {
            println!("{d:#?}");
        }
    }
}
