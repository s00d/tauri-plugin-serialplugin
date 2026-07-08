//! Probe table vs Android device_filter.xml.

use android_usb_serial::probe::{DriverType, ProbeTable};
use android_usb_serial::transport::InterfaceInfo;
use std::fs;
use std::path::Path;

fn parse_device_filter_xml(path: &Path) -> Vec<(u16, u16)> {
    let text = fs::read_to_string(path).expect("read device_filter.xml");
    let mut out = Vec::new();
    for line in text.lines() {
        if let (Some(v), Some(p)) = (
            line.split("vendor-id=\"")
                .nth(1)
                .and_then(|s| s.split('"').next()),
            line.split("product-id=\"")
                .nth(1)
                .and_then(|s| s.split('"').next()),
        ) {
            if let (Ok(vid), Ok(pid)) = (v.parse::<u16>(), p.parse::<u16>()) {
                out.push((vid, pid));
            }
        }
    }
    out
}

#[test]
fn device_filter_explicit_drivers_match_probe_table() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../android/src/main/res/xml/device_filter.xml");
    let table = ProbeTable::default_table();
    let pairs = parse_device_filter_xml(&root);
    assert!(!pairs.is_empty(), "device_filter.xml should list devices");

    for (vid, pid) in pairs {
        let driver = table.find(vid, pid, &[]);
        if driver == DriverType::CdcAcm {
            continue;
        }
        assert!(
            table
                .entries()
                .iter()
                .any(|e| e.vendor_id == vid && e.product_id == pid),
            "explicit driver {vid:04x}:{pid:04x} -> {driver:?} missing from ProbeTable"
        );
    }
}

#[test]
fn cdc_fallback_for_unknown_vid() {
    let table = ProbeTable::default_table();
    let ifaces = vec![
        InterfaceInfo {
            id: 0,
            class: 2,
            subclass: 2,
            protocol: 0,
        },
        InterfaceInfo {
            id: 1,
            class: 10,
            subclass: 0,
            protocol: 0,
        },
    ];
    assert_eq!(table.find(0x9999, 0x0001, &ifaces), DriverType::CdcAcm);
}

#[test]
fn probe_table_matches_device_filter_ftdi() {
    let table = ProbeTable::default_table();
    assert_eq!(table.find(0x0403, 0x6001, &[]), DriverType::Ftdi);
}

#[test]
fn cdc_only_filter_entries_use_fallback() {
    let table = ProbeTable::default_table();
    assert_eq!(table.find(0x2341, 0x0043, &[]), DriverType::CdcAcm);
}
