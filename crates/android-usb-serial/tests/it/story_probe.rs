//! Stories: devices as Android and the bus present them.

use super::common::*;

/// Given an IAD-class device whose descriptors carry a broken bLength (1, or one that runs past
/// the end) before the only IAD, which would pick interfaces 2 and 3
/// When port 0 opens
/// Then the scan stops at the broken descriptor and the interface order (0 and 1) wins
#[test]
fn a_device_with_broken_descriptors() {
    for bad in [vec![1u8], vec![40, 0x24]] {
        let fake = FakeTransport::cdc_multi();
        let mut raw = bad;
        raw.extend([8, 0x0B, 2, 2, 2, 2, 1, 0]);
        fake.set_raw_descriptors(raw);
        let _port = open_port(shared(&fake), 0).unwrap();
        let mut claimed = fake.claimed_interfaces();
        claimed.sort_unstable();
        assert_eq!(claimed, [0, 1]);
    }
}

/// Parse an id the way Android's DeviceFilter does: hex with `0x`, else decimal.
fn parse_android_id(v: &str) -> u16 {
    let parsed = match v.strip_prefix("0x").or_else(|| v.strip_prefix("0X")) {
        Some(hex) => u16::from_str_radix(hex, 16),
        None => v.parse(),
    };
    // Android ignores an unparsable id, which silently turns the row into a wildcard.
    parsed.unwrap_or_else(|_| panic!("device_filter.xml id {v:?} would be a wildcard"))
}

/// Given the Android app's `device_filter.xml`
/// When it is compared with the built-in probe table
/// Then every table entry is listed, every listed VID/PID has a driver, and no row without
/// vendor and product (or class) matches every device
#[test]
fn the_android_filter_lists_every_supported_device() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../android/src/main/res/xml/device_filter.xml");
    let text = std::fs::read_to_string(&path).expect("read device_filter.xml");
    let attr = |line: &str, name: &str| {
        line.split(&format!("{name}=\""))
            .nth(1)
            .and_then(|s| s.split('"').next())
            .map(parse_android_id)
    };
    let mut pairs = Vec::new();
    for line in text
        .lines()
        .filter(|l| l.trim_start().starts_with("<usb-device"))
    {
        match (attr(line, "vendor-id"), attr(line, "product-id")) {
            (Some(v), Some(p)) => pairs.push((v, p)),
            _ => assert!(line.contains("class="), "row matches anything: {line}"),
        }
    }
    let table = ProbeTable::default_table();
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
                .any(|e| (e.vendor_id, e.product_id) == (vid, pid)),
            "device_filter.xml lists {vid:04x}:{pid:04x}, which no driver handles"
        );
    }
}
