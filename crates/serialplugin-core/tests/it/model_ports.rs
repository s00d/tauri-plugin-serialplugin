//! Model tests of port lists: the 4.x info map, the typed details, one entry per device,
//! and the difference of two lists. The system's own list is checked against serialport.

use proptest::collection::{hash_map, hash_set, vec};
use proptest::option;
use proptest::prelude::*;
use proptest::test_runner::Config;
use serialplugin_core::port::details::{
    available_ports_detailed, details_from_info_map, details_from_type, PortDetails, PortType,
};
use serialplugin_core::port::list::{
    apply_android_single_port_per_device, apply_single_port_per_device, enumerate_available_ports,
    port_info_from_type,
};
use serialplugin_core::SerialManager;
use serialport::{SerialPortType, UsbPortInfo};
use std::collections::{HashMap, HashSet};

type Info = HashMap<String, String>;

fn info(kind: &str) -> Info {
    HashMap::from([("type".to_string(), kind.to_string())])
}

fn port_type() -> impl Strategy<Value = SerialPortType> {
    let text = || option::of("[A-Za-z0-9]{1,6}");
    prop_oneof![
        4 => (any::<u16>(), any::<u16>(), text(), text(), text(), any::<u8>()).prop_map(
            |(vid, pid, serial_number, manufacturer, product, _interface)| {
                SerialPortType::UsbPort(UsbPortInfo {
                    vid,
                    pid,
                    serial_number,
                    manufacturer,
                    product,
                    #[cfg(feature = "usb-port-details")]
                    location: Some(serialport::Location::new("1".into(), vec![2, 3])),
                    #[cfg(feature = "usb-port-details")]
                    interface: Some(_interface),
                })
            }
        ),
        1 => Just(SerialPortType::BluetoothPort),
        1 => Just(SerialPortType::PciPort),
        1 => Just(SerialPortType::Unknown),
    ]
}

/// Paths: callout / dial-in pairs, Linux, Windows.
fn paths() -> impl Strategy<Value = HashSet<String>> {
    let path = prop_oneof![
        "/dev/(cu|tty)\\.(usb|bt)[0-2]",
        "/dev/ttyUSB[0-2]",
        "COM[0-2]",
    ];
    hash_set(path, 0..8)
}

/// Android paths: `device#port`, or the device alone (port 0).
fn android_paths() -> impl Strategy<Value = HashSet<String>> {
    hash_set("/dev/bus/usb/001/00[1-3](#[0-3])?", 0..8)
}

fn with_info(paths: &HashSet<String>) -> HashMap<String, Info> {
    paths.iter().map(|p| (p.clone(), info("USB"))).collect()
}

fn keys(map: HashMap<String, Info>) -> HashSet<String> {
    map.into_keys().collect()
}

/// The `type` text of the 4.x map.
fn type_text(t: &PortType) -> &'static str {
    match t {
        PortType::Usb => "USB",
        PortType::Bluetooth => "Bluetooth",
        PortType::Pci => "PCI",
        _ => "Unknown",
    }
}

proptest! {
    #![proptest_config(Config { cases: 128, failure_persistence: None, ..Config::default() })]

    /// The 4.x map has the same six keys for every port (`Unknown` for what is not known;
    /// nothing that tells the interfaces of one device apart), the typed details the same
    /// values; a map turns back into the details.
    #[test]
    fn info_and_details_say_the_same(t in port_type()) {
        let map = port_info_from_type(t.clone());
        let d = details_from_type("/dev/x".into(), t.clone());
        let text = |v: &Option<String>| v.clone().unwrap_or_else(|| "Unknown".into());
        let number = |v: Option<u16>| v.map_or("Unknown".into(), |v| v.to_string());
        let want: Info = [
            ("type", type_text(&d.port_type).to_string()),
            ("vid", number(d.vid)),
            ("pid", number(d.pid)),
            ("serial_number", text(&d.serial_number)),
            ("manufacturer", text(&d.manufacturer)),
            ("product", text(&d.product)),
        ].into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        prop_assert_eq!(&map, &want);
        let mut details = PortDetails::default();
        details.path = "/dev/x".into();
        match &t {
            SerialPortType::UsbPort(u) => {
                details.port_type = PortType::Usb;
                details.vid = Some(u.vid);
                details.pid = Some(u.pid);
                details.serial_number = u.serial_number.clone();
                details.manufacturer = u.manufacturer.clone();
                details.product = u.product.clone();
                #[cfg(feature = "usb-port-details")]
                {
                    details.interface = u.interface;
                    details.location = Some("1-2.3".into());
                }
            }
            SerialPortType::BluetoothPort => details.port_type = PortType::Bluetooth,
            SerialPortType::PciPort => details.port_type = PortType::Pci,
            SerialPortType::Unknown => details.port_type = PortType::Unknown,
        }
        prop_assert_eq!(&d, &details);
        details.interface = None;
        details.location = None;
        prop_assert_eq!(details_from_info_map("/dev/x".into(), &map), details);
    }

    /// Android and iOS maps: numbers are decimal or `0x` hex, anything else is not known.
    #[test]
    fn details_read_hex_and_decimal_ids(vid in any::<u16>(), pid in any::<u16>(), junk in "[g-z]{0,3}") {
        let map = HashMap::from([
            ("vid".to_string(), format!("0x{vid:X}")),
            ("pid".to_string(), pid.to_string()),
        ]);
        let d = details_from_info_map("p".into(), &map);
        prop_assert_eq!((d.vid, d.pid), (Some(vid), Some(pid)));
        prop_assert_eq!(d.port_type, PortType::Unknown);
        let d = details_from_info_map("p".into(), &HashMap::from([("vid".to_string(), junk)]));
        prop_assert_eq!(d.vid, None);
    }

    /// Android's map of a device without USB permission: type `Usb`, empty strings. The details
    /// are a USB port's, and the strings that come with the permission are no other device.
    #[test]
    fn android_map_before_and_after_the_permission(serial in "[A-Z0-9]{1,8}") {
        let android = |serial: &str| -> Info {
            [("type", "Usb"), ("vid", "0x1A86"), ("pid", "0x7523"), ("manufacturer", ""),
             ("product", "USB Serial"), ("serial_number", serial)]
                .into_iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
        };
        let path = "/dev/bus/usb/001/002";
        let d = details_from_info_map(path.into(), &android(""));
        prop_assert_eq!(d.port_type, PortType::Usb);
        prop_assert_eq!((d.vid, d.pid), (Some(0x1A86), Some(0x7523)));
        prop_assert_eq!((d.manufacturer, d.serial_number), (None, None));
        prop_assert_eq!(d.product.as_deref(), Some("USB Serial"));

        let list = |serial: &str| HashMap::from([(path.to_string(), android(serial))]);
        use serialplugin_core::port::list_monitor::diff_ports;
        let (added, removed) = diff_ports(&list(""), &list(&serial));
        prop_assert!(added.is_empty() && removed.is_empty());
        let other = format!("{serial}X");
        prop_assert_eq!(diff_ports(&list(&serial), &list(&other)).1, vec![path.to_string()]);
    }

    /// One port per device: of a macOS pair the callout (`cu.`) one, a lone dial-in one
    /// stays, other systems' paths stay. Off, nothing changes.
    #[test]
    fn one_port_per_macos_device(paths in paths()) {
        let all = with_info(&paths);
        prop_assert_eq!(apply_single_port_per_device(all.clone(), false), all.clone());
        let want: HashSet<String> = paths.iter()
            .filter(|p| p.strip_prefix("/dev/tty.").is_none_or(|name| !paths.contains(&format!("/dev/cu.{name}"))))
            .cloned()
            .collect();
        let got = apply_single_port_per_device(all, true);
        prop_assert!(got.values().all(|i| *i == info("USB")));
        prop_assert_eq!(keys(got), want);
    }

    /// Of one Android device's ports the lowest index stays (no index is 0), whatever order
    /// the map yields them in.
    #[test]
    fn one_port_per_android_device(paths in android_paths()) {
        let all = with_info(&paths);
        prop_assert_eq!(apply_android_single_port_per_device(all.clone(), false), all.clone());
        let parts = |p: &str| match p.split_once('#') {
            Some((device, i)) => (device.to_string(), i.parse::<u32>().unwrap()),
            None => (p.to_string(), 0),
        };
        let got = keys(apply_android_single_port_per_device(all, true));
        let devices: HashSet<String> = paths.iter().map(|p| parts(p).0).collect();
        prop_assert_eq!(got.len(), devices.len());
        for p in &got {
            let (device, i) = parts(p);
            prop_assert!(paths.contains(p));
            prop_assert!(paths.iter().map(|p| parts(p)).all(|(d, j)| d != device || i <= j), "{}", p);
        }
    }

    /// `tcp://`, `ble://` and mux channels have no modem lines of their own.
    #[test]
    fn only_local_ports_have_modem_lines(base in "(/dev/tty[A-Z]{1,4}|COM)[0-9]", dlci in 1u8..=63, host in "[a-z0-9.:]{1,12}") {
        let m = SerialManager::new();
        prop_assert!(m.supports_modem_lines(&base));
        for path in [format!("{base}#dlci={dlci}"), format!("tcp://{host}"), format!("ble://{host}"), format!("tcp://{host}#dlci={dlci}")] {
            prop_assert!(!m.supports_modem_lines(&path), "{}", path);
        }
    }

    /// Two lists differ in exactly the paths added and removed, and in those whose known
    /// identity changed (both).
    #[test]
    fn a_list_diff_turns_the_old_list_into_the_new(
        old in hash_map("/dev/[a-c]", listed(), 0..4), new in hash_map("/dev/[a-c]", listed(), 0..4)
    ) {
        use serialplugin_core::port::list_monitor::diff_ports;
        let (added, removed) = diff_ports(&old, &new);
        let same = |p: &String| old.get(p).zip(new.get(p)).is_some_and(|(o, n)| {
            ["type", "vid"].iter().all(|k| o[*k] == n[*k] || o[*k] == "Unknown" || n[*k] == "Unknown")
        });
        let mut want_removed: Vec<_> = old.keys().filter(|p| !same(p)).cloned().collect();
        let mut want_added: Vec<_> = new.keys().filter(|p| !same(p)).cloned().collect();
        want_removed.sort();
        want_added.sort();
        prop_assert_eq!(removed, want_removed);
        prop_assert_eq!(added.iter().map(|(p, _)| p.clone()).collect::<Vec<_>>(), want_added);
        prop_assert!(added.iter().all(|(p, i)| new[p] == *i));
    }
}

fn listed() -> impl Strategy<Value = Info> {
    let value = || prop::sample::select(vec!["Unknown", "1", "2"]);
    vec(value(), 2).prop_map(|v| {
        HashMap::from([
            ("type".to_string(), v[0].to_string()),
            ("vid".to_string(), v[1].to_string()),
        ])
    })
}

/// Given the ports serialport enumerates on this machine,
/// when the 4.x map and the typed list are asked for (through the manager and directly),
/// then they hold exactly those ports, the typed list sorted by path.
#[test]
fn the_lists_hold_the_system_ports() {
    let mut raw: Vec<String> = serialport::available_ports()
        .unwrap()
        .into_iter()
        .map(|p| p.port_name)
        .collect();
    raw.sort();
    let m = SerialManager::new();
    for list in [
        m.available_ports_detailed().unwrap(),
        available_ports_detailed().unwrap(),
    ] {
        let paths: Vec<_> = list.into_iter().map(|d| d.path).collect();
        assert_eq!(paths, raw);
    }
    for map in [
        m.available_ports(false).unwrap(),
        enumerate_available_ports(false).unwrap(),
    ] {
        assert!(map.values().all(|info| info.len() == 6));
        let mut paths: Vec<_> = map.into_keys().collect();
        paths.sort();
        assert_eq!(paths, raw);
    }
}
