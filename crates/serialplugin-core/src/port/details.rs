//! Typed per-port details (`available_ports_detailed`): numeric vid/pid, USB
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
    #[cfg(feature = "usb-port-details")]
    let location = usb_location(&port);
    match port {
        T::UsbPort(info) => {
            #[allow(unused_mut)]
            let mut d = PortDetails {
                path,
                port_type: PortType::Usb,
                vid: Some(info.vid),
                pid: Some(info.pid),
                serial_number: info.serial_number,
                manufacturer: info.manufacturer,
                product: info.product,
                ..Default::default()
            };
            // Without the feature serialport has no interface / location: `None`.
            #[cfg(feature = "usb-port-details")]
            {
                d.interface = info.interface;
                d.location = location;
            }
            d
        }
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

/// Convert a 4.x string map (Android, iOS): `"Unknown"` and empty -> `None`, vid/pid hex
/// (`0x1A86`) or decimal; Android's type is `Usb`. No interface / location there.
pub fn details_from_info_map(
    path: String,
    info: &std::collections::HashMap<String, String>,
) -> PortDetails {
    let text = |k: &str| {
        info.get(k)
            .filter(|v| !v.is_empty() && v.as_str() != crate::state::UNKNOWN)
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
        Some(t) if t.eq_ignore_ascii_case(crate::state::USB) => PortType::Usb,
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

#[cfg(all(test, unix))]
mod tests {
    //! In `src/`: `/dev/serial/by-*` belongs to the system; the reader is given a directory.
    use super::*;

    #[test]
    fn serial_links_maps_targets_to_link_names() {
        // Removed on drop too, so a failed assert (or a mutant) leaves no temp dir behind.
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        // Next to the test binary (inside cargo's target dir), not the system temp dir.
        let exe = std::env::current_exe().unwrap();
        let root = exe
            .parent()
            .unwrap()
            .join(format!("sp-links-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let _cleanup = Cleanup(root.clone());
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
}
