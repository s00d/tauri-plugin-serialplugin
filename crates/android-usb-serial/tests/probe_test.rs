//! Probe table vs Android device_filter.xml.

use android_usb_serial::probe::{DriverType, ProbeTable};
use android_usb_serial::transport::InterfaceInfo;
use std::fs;
use std::path::Path;

/// Parse an id the way Android's DeviceFilter does: hex with `0x`, else decimal.
fn parse_android_id(v: &str) -> u16 {
    let parsed = match v.strip_prefix("0x").or_else(|| v.strip_prefix("0X")) {
        Some(hex) => u16::from_str_radix(hex, 16),
        None => v.parse(),
    };
    // Android ignores an unparsable id, which silently turns the row into a wildcard.
    parsed.unwrap_or_else(|_| panic!("device_filter.xml id {v:?} would be a wildcard"))
}

fn parse_device_filter_xml(path: &Path) -> Vec<(u16, u16)> {
    let text = fs::read_to_string(path).expect("read device_filter.xml");
    let attr = |line: &str, name: &str| {
        line.split(&format!("{name}=\""))
            .nth(1)
            .and_then(|s| s.split('"').next())
            .map(parse_android_id)
    };
    let mut out = Vec::new();
    for line in text
        .lines()
        .filter(|l| l.trim_start().starts_with("<usb-device"))
    {
        let has_class = line.contains("class=");
        match (attr(line, "vendor-id"), attr(line, "product-id")) {
            (Some(v), Some(p)) => out.push((v, p)),
            _ => assert!(
                has_class,
                "row without vendor+product or class matches anything: {line}"
            ),
        }
    }
    out
}

#[test]
fn device_filter_matches_probe_table() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../android/src/main/res/xml/device_filter.xml");
    let table = ProbeTable::default_table();
    let pairs = parse_device_filter_xml(&root);
    for e in table.entries() {
        assert!(
            pairs.contains(&(e.vendor_id, e.product_id)),
            "{:04x}:{:04x} is in ProbeTable but not in device_filter.xml",
            e.vendor_id,
            e.product_id
        );
    }
    for (vid, pid) in pairs {
        assert!(
            table
                .entries()
                .iter()
                .any(|e| e.vendor_id == vid && e.product_id == pid),
            "device_filter.xml lists {vid:04x}:{pid:04x}, which no driver handles"
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
