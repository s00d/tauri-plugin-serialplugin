//! Probe properties: generated descriptor sets (known and unknown VID/PID, CDC
//! functions with and without IAD, extra non-serial interfaces) must probe to the
//! right driver, port count and interfaces; arbitrary descriptor bytes must give
//! no panic and no hang.

use super::common::*;

fn interfaces() -> impl Strategy<Value = Vec<InterfaceInfo>> {
    let class = prop_oneof![Just(2u8), Just(10), Just(255), Just(8), any::<u8>()];
    let subclass = prop_oneof![Just(2u8), any::<u8>()];
    prop::collection::vec((class, subclass), 0..7).prop_map(|v| {
        v.into_iter()
            .enumerate()
            .map(|(i, (c, s))| iface(i as u8, c, s))
            .collect()
    })
}

fn vid_pid() -> impl Strategy<Value = (u16, u16)> {
    let known: Vec<(u16, u16)> = ProbeTable::default_table()
        .entries()
        .iter()
        .map(|e| (e.vendor_id, e.product_id))
        .collect();
    prop_oneof![prop::sample::select(known), any::<(u16, u16)>()]
}

#[test]
fn describe_picks_driver_and_port_count() {
    let any_driver = prop::sample::select(vec![
        DriverType::CdcAcm,
        DriverType::Ftdi,
        DriverType::Cp21xx,
        DriverType::Ch34x,
        DriverType::Prolific,
        DriverType::GsmModem,
        DriverType::ChromeCcd,
    ]);
    run_cases(
        512,
        (vid_pid(), interfaces(), any_driver),
        |((vid, pid), ifaces, arg)| {
            let fake = FakeTransport::cdc_single_iface();
            fake.set_vendor_product(vid, pid);
            fake.set_interfaces(ifaces.clone());
            let dev = describe_device(&shared(&fake)).unwrap();
            // The VID/PID table itself is pinned by the golden probe-table fixture.
            let listed = ProbeTable::default_table()
                .entries()
                .iter()
                .find(|e| e.vendor_id == vid && e.product_id == pid)
                .map(|e| e.driver);
            let driver = listed.unwrap_or(DriverType::CdcAcm);
            let comm = ifaces
                .iter()
                .filter(|i| i.class == 2 && i.subclass == 2)
                .count();
            let data = ifaces.iter().filter(|i| i.class == 10).count();
            // ECM / RNDIS: a data interface without an ACM comm one is no serial port.
            let count = |driver, n_ifaces: usize, cdc: usize| match driver {
                DriverType::CdcAcm => cdc,
                DriverType::Ftdi | DriverType::Cp21xx => n_ifaces.max(1),
                DriverType::ChromeCcd => 3,
                _ => 1,
            };
            let ports = count(driver, ifaces.len(), comm.min(data)).max(1);
            let table = ProbeTable::default_table();
            prop_assert_eq!(
                table.port_count(arg, &ifaces),
                count(arg, ifaces.len(), comm.min(data))
            );
            // Before the interfaces are known, multi-port products count by VID/PID alone.
            let by_product = match (vid, pid) {
                (0x0403, 0x6011) => 4,
                (0x0403, 0x6010 | 0x6014 | 0x6015) | (0x10C4, 0xEA70 | 0xEA71) => 2,
                (0x18D1, 0x5014) => 3,
                _ => count(arg, 0, 0),
            };
            prop_assert_eq!(table.port_count_product(vid, pid, arg, &[]), by_product);
            if !ifaces.is_empty() {
                prop_assert_eq!(
                    table.port_count_product(vid, pid, arg, &ifaces),
                    table.port_count(arg, &ifaces)
                );
            }
            prop_assert_eq!((dev.vendor_id, dev.product_id), (vid, pid));
            prop_assert_eq!(dev.driver, driver);
            prop_assert_eq!(dev.ports.len(), ports);
            for (i, p) in dev.ports.iter().enumerate() {
                prop_assert_eq!(
                    (p.port_index, p.driver, p.vendor_id, p.product_id),
                    (i, driver, vid, pid)
                );
            }
            Ok(())
        },
    );
}

#[derive(Debug, Clone)]
enum Function {
    /// CDC ACM comm + data interface pair; `true` = comm has a notification endpoint.
    Cdc(bool),
    /// A non-serial interface (vendor, mass storage, audio, CDC-ECM comm …).
    Other(u8, u8),
}

fn functions() -> impl Strategy<Value = Vec<Function>> {
    let f = prop_oneof![
        3 => any::<bool>().prop_map(Function::Cdc),
        1 => prop::sample::select(vec![(1u8, 0u8), (3, 0), (8, 0), (0x0E, 0), (255, 0), (2, 6)])
            .prop_map(|(c, s)| Function::Other(c, s)),
    ];
    prop::collection::vec(f, 1..6).prop_filter("at least one CDC port", |v| {
        v.iter().any(|f| matches!(f, Function::Cdc(_)))
    })
}

struct Layout {
    fake: FakeTransport,
    /// (comm, data, data bulk-OUT endpoint) per CDC port, in port order.
    ports: Vec<(u8, u8, u8)>,
}

const IAD_CLASS: [u8; 3] = [0xEF, 0x02, 0x01];

fn build(functions: &[Function], class: [u8; 3], reverse_iad: bool) -> Layout {
    let fake = FakeTransport::cdc_single_iface();
    let mut ifaces = vec![];
    let mut eps: Vec<(u8, Vec<EndpointInfo>)> = vec![];
    let mut pairs = vec![];
    let mut iads: Vec<Vec<u8>> = vec![];
    let mut body = vec![];
    let mut id = 0u8;
    for f in functions {
        match *f {
            Function::Cdc(notify) => {
                let (c, d) = (id, id + 1);
                ifaces.push(iface(c, 2, 2));
                ifaces.push(iface(d, 10, 0));
                let comm_eps = if notify {
                    vec![ep(0x80 | (c + 1), 3)]
                } else {
                    vec![]
                };
                let out = d + 1;
                eps.push((c, comm_eps));
                eps.push((d, vec![ep(0x80 | (d + 1), 2), ep(out, 2)]));
                pairs.push((c, d, out));
                iads.push(vec![8, 0x0B, c, 2, 2, 2, 1, 0]);
                body.extend([9, 4, c, 0, u8::from(notify), 2, 2, 1, 0]);
                body.extend([9, 4, d, 0, 2, 10, 0, 0, 0]);
                id += 2;
            }
            Function::Other(class, sub) => {
                ifaces.push(iface(id, class, sub));
                eps.push((id, vec![ep(0x80 | (id + 1), 2)]));
                // Non-ACM (or single-interface) associations must be skipped.
                iads.push(vec![8, 0x0B, id, 1, class, sub, 0, 0]);
                body.extend([9, 4, id, 0, 1, class, sub, 0, 0]);
                id += 1;
            }
        }
    }
    if reverse_iad {
        iads.reverse();
        pairs.reverse();
    }
    let mut raw = vec![9, 2, 0, 0, ifaces.len() as u8, 1, 0, 0x80, 50];
    for d in &iads {
        raw.extend(d);
    }
    raw.extend(body);
    fake.set_interfaces(ifaces);
    fake.configure_endpoints(&eps);
    fake.set_raw_descriptors(raw);
    fake.patch_device_descriptor(|d| d[4..7].copy_from_slice(&class));
    Layout { fake, ports: pairs }
}

#[test]
fn cdc_probe_opens_the_right_interface_pair() {
    // The IAD class triple, all three bytes off, or one byte off (not an IAD device).
    let class = prop_oneof![
        Just(IAD_CLASS),
        Just([0u8; 3]),
        (0usize..3).prop_map(|i| {
            let mut c = IAD_CLASS;
            c[i] = 0;
            c
        }),
    ];
    run_cases(
        128,
        (functions(), class, any::<bool>()),
        |(functions, class, reverse)| {
            // IAD order only matters on an IAD-class device; otherwise interface order rules.
            let layout = build(&functions, class, reverse && class == IAD_CLASS);
            let t = shared(&layout.fake);
            let dev = describe_device(&t).unwrap();
            prop_assert_eq!(dev.driver, DriverType::CdcAcm);
            prop_assert_eq!(dev.ports.len(), layout.ports.len());
            for (i, &(comm, data, out)) in layout.ports.iter().enumerate() {
                let mut port = open_port(t.clone(), i)
                    .map_err(|e| TestCaseError::fail(format!("port {i}: {e:?}")))?;
                let mut claimed = layout.fake.claimed_interfaces();
                claimed.sort_unstable();
                prop_assert_eq!(claimed, vec![comm, data], "port {}", i);
                layout.fake.clear_recorded();
                port.write(b"x").unwrap();
                prop_assert_eq!(layout.fake.recorded_bulk_out()[0].endpoint, out);
                port.set_dtr(true).unwrap();
                prop_assert_eq!(layout.fake.recorded_controls()[0].index, u16::from(comm));
                drop(port);
                prop_assert!(layout.fake.claimed_interfaces().is_empty());
            }
            prop_assert!(
                open_port(t, layout.ports.len()).is_err(),
                "port past the end"
            );
            Ok(())
        },
    );
}

/// Descriptor-shaped noise: real IAD/interface headers with hostile fields, bad
/// lengths, and raw bytes.
fn descriptor_bytes() -> impl Strategy<Value = Vec<u8>> {
    let first = prop_oneof![Just(255u8), Just(254), any::<u8>()];
    let iad = (
        first,
        any::<u8>(),
        prop_oneof![Just(2u8), any::<u8>()],
        prop_oneof![Just(2u8), any::<u8>()],
    )
        .prop_map(|(first, count, class, sub)| vec![8, 0x0B, first, count, class, sub, 0, 0]);
    let generic = (
        0u8..24,
        any::<u8>(),
        prop::collection::vec(any::<u8>(), 0..24),
    )
        .prop_map(|(len, ty, mut body)| {
            let mut d = vec![len, ty];
            d.append(&mut body);
            d
        });
    let noise = prop::collection::vec(any::<u8>(), 0..40);
    prop::collection::vec(prop_oneof![3 => iad, 2 => generic, 1 => noise], 0..8)
        .prop_map(|v| v.concat())
}

#[test]
fn arbitrary_descriptors_never_panic() {
    run_cases(
        512,
        (descriptor_bytes(), interfaces(), any::<bool>()),
        |(raw, ifaces, iad)| {
            let fake = FakeTransport::cdc_single_iface();
            fake.set_interfaces(ifaces.clone());
            let eps: Vec<(u8, Vec<EndpointInfo>)> = ifaces
                .iter()
                .map(|i| (i.id, vec![ep(0x81 + i.id, 2), ep(0x01 + i.id, 2)]))
                .collect();
            fake.configure_endpoints(&eps);
            fake.set_raw_descriptors(raw);
            if iad {
                fake.patch_device_descriptor(|d| d[4..7].copy_from_slice(&[0xEF, 0x02, 0x01]));
            }
            let t = shared(&fake);
            let dev = describe_device(&t).unwrap();
            for i in 0..=dev.ports.len() {
                let _ = open_port(t.clone(), i);
                prop_assert!(
                    fake.claimed_interfaces().is_empty(),
                    "port {} leaked a claim",
                    i
                );
            }
            Ok(())
        },
    );
}

/// Given an IAD-class device whose descriptors carry a broken bLength before the
///       only IAD (which would pick interfaces 2/3)
/// When port 0 opens
/// Then the scan stops at the broken descriptor and the interface order (0/1) wins.
#[test]
fn iad_scan_stops_at_a_bad_descriptor_length() {
    for bad in [vec![1u8], vec![40, 0x24]] {
        let fake = FakeTransport::cdc_multi();
        // bLength 1, or a bLength 40 that runs past the end of the buffer.
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
/// Then every table entry is listed, every listed VID/PID has a driver, and no row
///      without vendor+product (or class) matches every device.
#[test]
fn device_filter_matches_the_probe_table() {
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
