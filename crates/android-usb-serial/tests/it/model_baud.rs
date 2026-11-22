//! Model: baud rates per chip.
//!
//! An accepted rate decodes back within the chip's tolerance, the decoded rate is monotonic in
//! the request, and only rates the chip cannot reach are rejected. CH34x, PL2303 and CP21xx
//! encodings are read from the control transfers the driver sends to `FakeTransport`.

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
fn ftdi_divisors() {
    run_cases(
        512,
        (baud(), 0usize..4, 1u32..1_750_000),
        |(baud, port, other)| {
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
            // A higher rate never has a larger divisor.
            let (lo, hi) = (baud.min(other), baud.max(other));
            if let (true, Ok((v1, i1)), Ok((v2, i2))) = (
                hi < 1_750_000,
                ftdi_baud_encoding(lo, false, 0),
                ftdi_baud_encoding(hi, false, 0),
            ) {
                prop_assert!(ftdi_eighths(v1, i1) >= ftdi_eighths(v2, i2));
            }
            Ok(())
        },
    );
}

/// A sent control transfer: (bRequest, wValue, wIndex, data).
type Sent = (u8, u16, u16, Vec<u8>);

/// Set `baud` (8N1) on a fresh line and return the transfers it produced, or `None` if rejected
/// (nothing may be sent then).
fn set_baud(
    fake: &FakeTransport,
    port: &mut SerialPortHandle,
    baud: u32,
) -> Result<Option<Vec<Sent>>, TestCaseError> {
    fake.clear_recorded();
    let cfg = LineConfig {
        baud_rate: baud,
        ..LineConfig::default()
    };
    let ok = port.set_line_config(cfg).is_ok();
    let sent: Vec<Sent> = fake
        .recorded_controls()
        .into_iter()
        .map(|c| (c.request, c.value, c.index, c.data))
        .collect();
    prop_assert!(ok || sent.is_empty(), "rejected {} sent {:?}", baud, sent);
    Ok(ok.then_some(sent))
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

/// What a driver's transfers say about the rate: the rate itself, or a divisor's effective rate.
enum Rate {
    Itself(u32),
    Divisor(f64),
    /// A dedicated divisor off the formula.
    Dedicated,
}

fn rate(chip: Chip, sent: &[Sent]) -> Rate {
    match chip.family() {
        Family::Ch => ch_effective(sent).map_or(Rate::Dedicated, Rate::Divisor),
        Family::Pl(t) => {
            let field = pl_field(sent);
            let divisor = pl_effective(field, t == PlType::Ta).filter(|_| t != PlType::Hxn);
            divisor.map_or(Rate::Itself(field), Rate::Divisor)
        }
        // CP21xx SET_BAUDRATE comes first.
        _ => Rate::Itself(u32::from_le_bytes(sent[0].3[..4].try_into().unwrap())),
    }
}

/// Rates the chip has no way to make.
fn unreachable_rate(chip: Chip, baud: u32) -> bool {
    match chip.family() {
        Family::Ch => baud <= 45 || baud > 40_000_000,
        // Bit 31 would read as a divisor.
        Family::Pl(PlType::Hxn) => baud == 0 || baud >= 1 << 31,
        Family::Pl(PlType::Ta) => !(6..=11_000_000).contains(&baud),
        Family::Pl(_) => !(46..=11_000_000).contains(&baud),
        _ => baud == 0,
    }
}

#[test]
fn sent_rates_are_the_requested_ones() {
    let chips = [
        Chip::Ch340,
        Chip::Cp2102,
        Chip::Pl2303(PlType::Hx),
        Chip::Pl2303(PlType::T01),
        Chip::Pl2303(PlType::Ta),
        Chip::Pl2303(PlType::Hxn),
    ];
    for chip in chips {
        run_cases(128, (baud(), baud()), |(a, b)| {
            let fake = chip.fake();
            let mut port = open_port(shared(&fake), 0).expect("open");
            let mut seen = vec![];
            for baud in [a, b] {
                let Some(sent) = set_baud(&fake, &mut port, baud)? else {
                    prop_assert!(
                        unreachable_rate(chip, baud),
                        "{:?}: rejected {}",
                        chip,
                        baud
                    );
                    continue;
                };
                prop_assert!(baud > 0);
                if sent.is_empty() {
                    // PL2303: the same encoded line as before, nothing to send.
                    prop_assert!(matches!(chip.family(), Family::Pl(_)));
                    continue;
                }
                let standard = PL_STANDARD.contains(&baud);
                match rate(chip, &sent) {
                    Rate::Itself(field) => {
                        prop_assert_eq!(field, baud);
                        let divides = matches!(chip.family(), Family::Pl(t) if t != PlType::Hxn);
                        prop_assert!(!divides || standard, "{:?}: {} sent as is", chip, baud);
                    }
                    Rate::Divisor(eff) => {
                        prop_assert!(
                            deviation(eff, baud) < TOLERANCE,
                            "{:?}: {} → {}",
                            chip,
                            baud,
                            eff
                        );
                        prop_assert!(!standard || chip == Chip::Ch340);
                        seen.push((baud, eff));
                    }
                    Rate::Dedicated => prop_assert!(baud == 921_600 || baud == 307_200),
                }
            }
            if let [(b1, e1), (b2, e2)] = seen[..] {
                let monotonic = (b1 <= b2) == (e1 <= e2) || e1 == e2;
                prop_assert!(monotonic, "{:?}: {}→{} vs {}→{}", chip, b1, e1, b2, e2);
            }
            Ok(())
        });
    }
}
