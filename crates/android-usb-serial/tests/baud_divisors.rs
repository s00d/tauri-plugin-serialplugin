//! Baud divisor properties per chip: an accepted rate decodes back within the
//! chip's tolerance, the decoded rate is monotonic in the request, and rates the
//! chip cannot reach are rejected. CH34x / PL2303 / CP21xx encodings are read
//! from the control transfers the driver sends to `FakeTransport`.

use super::common::*;
use android_usb_serial::port::SerialPortHandle;

const TOLERANCE: f64 = 0.031;

fn deviation(effective: f64, baud: u32) -> f64 {
    (1.0 - effective / f64::from(baud)).abs()
}

/// FTDI divisor `n` in eighths: 3 MHz / (n / 8).
fn ftdi_eighths(value: u16, index: u16) -> u32 {
    let sub = match (value >> 14, index & 1) {
        (0, 0) => 0,
        (1, 0) => 4,
        (2, 0) => 2,
        (3, 0) => 1,
        (0, 1) => 3,
        (1, 1) => 5,
        (2, 1) => 6,
        _ => 7,
    };
    (u32::from(value & 0x3fff) << 3) + sub
}

#[test]
fn ftdi_divisor_round_trips_within_tolerance() {
    run_cases(512, (baud(), 0usize..4), |(baud, port)| {
        let plain = ftdi_baud_encoding(baud, false, 0);
        // The port only ever adds wIndex = (sub-divisor bit << 8) + port + 1.
        let with_port = ftdi_baud_encoding(baud, true, port);
        prop_assert_eq!(
            with_port.as_ref().ok().copied(),
            plain
                .as_ref()
                .ok()
                .map(|&(v, i)| (v, (i << 8) + port as u16 + 1))
        );
        prop_assert_eq!(plain.is_err(), with_port.is_err());
        if baud >= 2_500_000 {
            prop_assert_eq!(plain.ok(), Some((0, 0)), "3 Mbaud special divisor");
        } else if baud >= 1_750_000 {
            prop_assert_eq!(plain.ok(), Some((1, 0)), "2 Mbaud special divisor");
        } else {
            // Spec: the nearest 14.3-bit divisor n (3 MHz / (n / 8)); the chip's rate,
            // rounded to a whole baud, may miss the request by under 3.1%.
            let ideal = (24e6 / f64::from(baud.max(1))).round();
            let whole_baud = |n: f64| (24e6 / n).round();
            let reachable = baud > 0
                && ideal <= f64::from((0x3fff << 3) + 7)
                && deviation(whole_baud(ideal), baud) < TOLERANCE;
            match plain {
                Ok((v, i)) => {
                    prop_assert!(reachable);
                    prop_assert_eq!(i & !1, 0);
                    let n = ftdi_eighths(v, i);
                    prop_assert_eq!(f64::from(n), ideal);
                    prop_assert!(deviation(whole_baud(f64::from(n)), baud) < TOLERANCE);
                }
                Err(_) => prop_assert!(!reachable, "{} is reachable", baud),
            }
        }
        Ok(())
    });
}

#[test]
fn ftdi_divisor_is_monotonic() {
    run_cases(512, (1u32..1_750_000, 1u32..1_750_000), |(a, b)| {
        let (lo, hi) = (a.min(b), a.max(b));
        if let (Ok((v1, i1)), Ok((v2, i2))) = (
            ftdi_baud_encoding(lo, false, 0),
            ftdi_baud_encoding(hi, false, 0),
        ) {
            prop_assert!(ftdi_eighths(v1, i1) >= ftdi_eighths(v2, i2));
        }
        Ok(())
    });
}

/// A sent control transfer: (bRequest, wValue, wIndex, data).
type Sent = (u8, u16, u16, Vec<u8>);

fn open(fake: &FakeTransport) -> SerialPortHandle {
    let t: Arc<dyn Transport> = Arc::new(fake.clone());
    let port = open_port(t, 0).expect("open");
    fake.clear_recorded();
    port
}

/// Set `baud` (8N1) and return the transfers it produced, or `None` if rejected.
fn set_baud(fake: &FakeTransport, port: &mut SerialPortHandle, baud: u32) -> Option<Vec<Sent>> {
    fake.clear_recorded();
    let cfg = LineConfig {
        baud_rate: baud,
        ..LineConfig::default()
    };
    let ok = port.set_line_config(cfg).is_ok();
    let sent: Vec<_> = fake
        .recorded_controls()
        .into_iter()
        .map(|c| (c.request, c.value, c.index, c.data))
        .collect();
    if !ok {
        assert!(sent.is_empty(), "rejected rate {baud} still sent {sent:?}");
    }
    ok.then_some(sent)
}

const CH_F: f64 = 1_532_620_800.0;

/// CH34x effective rate from the 0x1312 / 0x0F2C register writes.
fn ch_effective(sent: &[Sent]) -> Option<f64> {
    let v1 = sent.iter().find(|c| c.0 == 0x9A && c.1 == 0x1312)?.2;
    let v2 = sent.iter().find(|c| c.0 == 0x9A && c.1 == 0x0F2C)?.2;
    assert_eq!(v1 & 0xf8, 0x80, "prescaler byte {v1:#06x}");
    assert_eq!(v2 & 0xff00, 0, "0x0F2C value {v2:#06x}");
    let div = u32::from(v1 & 0x07);
    if div > 3 {
        return None; // dedicated divisor, not on the formula
    }
    let factor = 0x10000 - ((u32::from(v1) & 0xff00) | u32::from(v2));
    Some(CH_F / (f64::from(factor) * f64::from(1u32 << (3 * (3 - div)))))
}

const PL_STANDARD: &[u32] = &[
    75, 150, 300, 600, 1200, 1800, 2400, 3600, 4800, 7200, 9600, 14400, 19200, 28800, 38400, 57600,
    115200, 230400, 460800, 614400, 921600, 1228800, 2457600, 3000000, 6000000,
];

/// PL2303 SET_LINE baud field → `Some(effective)` for a divisor, `None` for a direct value.
fn pl_effective(field: u32, type_t: bool) -> Option<f64> {
    if field & (1 << 31) == 0 {
        return None;
    }
    let base = 384_000_000u64;
    let (mantissa, shift) = if type_t {
        (field & 0x7ff, ((field >> 12) & 0xe) | ((field >> 16) & 1))
    } else {
        (field & 0x1ff, ((field >> 9) & 7) * 2)
    };
    assert!(mantissa > 0, "zero mantissa in {field:#x}");
    Some(((base / u64::from(mantissa)) >> shift) as f64)
}

fn pl_field(sent: &[Sent]) -> u32 {
    let line = &sent.iter().find(|c| c.0 == 0x20).expect("SET_LINE").3;
    u32::from_le_bytes([line[0], line[1], line[2], line[3]])
}

fn pl2303(kind: u8) -> FakeTransport {
    match kind {
        0 => {
            let f = FakeTransport::pl2303_hx();
            f.patch_device_descriptor(|d| {
                d[2] = 0x10;
                d[3] = 0x01;
            });
            f
        }
        1 => FakeTransport::pl2303_type01(),
        2 => FakeTransport::pl2303_ta(),
        _ => FakeTransport::pl2303_hxn(),
    }
}

#[test]
fn ch34x_divisor_round_trips_and_is_monotonic() {
    run_cases(256, (baud(), baud()), |(a, b)| {
        let fake = FakeTransport::ch340_dual_iface();
        let mut port = open(&fake);
        let mut seen = vec![];
        for baud in [a, b] {
            match set_baud(&fake, &mut port, baud) {
                Some(sent) => {
                    if let Some(eff) = ch_effective(&sent) {
                        prop_assert!(deviation(eff, baud) < TOLERANCE, "{} → {}", baud, eff);
                        seen.push((baud, eff));
                    } else {
                        prop_assert!(baud == 921_600 || baud == 307_200);
                    }
                }
                None => prop_assert!(baud <= 45 || baud > 40_000_000, "rejected {}", baud),
            }
        }
        if let [(b1, e1), (b2, e2)] = seen[..] {
            prop_assert!(
                (b1 <= b2) == (e1 <= e2) || e1 == e2,
                "{}→{} vs {}→{}",
                b1,
                e1,
                b2,
                e2
            );
        }
        Ok(())
    });
}

#[test]
fn pl2303_divisor_round_trips_and_is_monotonic() {
    run_cases(256, (0u8..4, baud(), baud()), |(kind, a, b)| {
        let fake = pl2303(kind);
        let mut port = open(&fake);
        let (type_t, hxn) = (kind == 2, kind == 3);
        let lowest = if type_t { 6 } else { 46 };
        let mut seen = vec![];
        for baud in [a, b] {
            fake.clear_recorded();
            let cfg = LineConfig {
                baud_rate: baud,
                ..LineConfig::default()
            };
            let ok = port.set_line_config(cfg).is_ok();
            let sent: Vec<_> = fake
                .recorded_controls()
                .into_iter()
                .map(|c| (c.request, c.value, c.index, c.data))
                .collect();
            if !ok {
                prop_assert!(sent.is_empty());
                let unreachable = if hxn {
                    baud == 0 || baud >= 1 << 31 // bit 31 would read as a divisor
                } else {
                    baud < lowest || baud > 11_000_000
                };
                prop_assert!(unreachable, "rejected {}", baud);
                continue;
            }
            if sent.is_empty() {
                continue; // same encoded line as before: nothing to send
            }
            let field = pl_field(&sent);
            if hxn {
                prop_assert_eq!(field, baud, "HXN takes the rate itself");
                continue;
            }
            match pl_effective(field, type_t) {
                Some(eff) => {
                    prop_assert!(!PL_STANDARD.contains(&baud));
                    prop_assert!(deviation(eff, baud) < TOLERANCE, "{} → {}", baud, eff);
                    seen.push((baud, eff));
                }
                None => {
                    prop_assert_eq!(field, baud);
                    prop_assert!(PL_STANDARD.contains(&baud));
                }
            }
        }
        if let [(b1, e1), (b2, e2)] = seen[..] {
            prop_assert!(
                (b1 <= b2) == (e1 <= e2) || e1 == e2,
                "{}→{} vs {}→{}",
                b1,
                e1,
                b2,
                e2
            );
        }
        Ok(())
    });
}

#[test]
fn cp21xx_sends_the_rate_itself() {
    run_cases(256, baud(), |baud| {
        let fake = FakeTransport::cp2102();
        let mut port = open(&fake);
        match set_baud(&fake, &mut port, baud) {
            Some(sent) => {
                prop_assert!(baud > 0);
                prop_assert_eq!(&sent[0], &(0x1E, 0, 0, baud.to_le_bytes().to_vec()));
            }
            None => prop_assert_eq!(baud, 0),
        }
        Ok(())
    });
}
