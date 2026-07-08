//! FTDI / XonXoff RX filter integration — all rx_filter fixtures.

use android_usb_serial::rx_filter::{strip_ftdi_header, RxFilter, XonXoffRxFilter};
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
struct RxFilterFixture {
    filter: String,
    input: String,
    output: String,
}

fn decode_hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
        .collect()
}

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rx_filter")
}

#[test]
fn all_rx_filter_fixture_pairs() {
    let dir = fixture_dir();
    let mut count = 0usize;
    for entry in fs::read_dir(&dir).expect("rx_filter dir") {
        let path = entry.expect("entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let text = fs::read_to_string(&path).unwrap();
        let fx: RxFilterFixture = serde_json::from_str(&text).unwrap();
        let input = decode_hex(&fx.input);
        let want = decode_hex(&fx.output);
        let got = match fx.filter.as_str() {
            "ftdi_header" => strip_ftdi_header(&input, 64),
            "xon_xoff" => {
                let mut f = XonXoffRxFilter::new(true);
                f.filter(&input)
            }
            other => panic!("unknown filter {other} in {}", path.display()),
        };
        assert_eq!(got, want, "fixture {}", path.display());
        count += 1;
    }
    assert!(count >= 6, "expected all rx_filter fixtures");
}

#[test]
fn strips_two_byte_status_per_packet() {
    let packet = vec![0u8, 0u8, b'H', b'i'];
    let out = strip_ftdi_header(&packet, 64);
    assert_eq!(out, b"Hi");
}

#[test]
fn status_only_packet_yields_empty() {
    let packet = vec![0u8, 0u8];
    assert!(strip_ftdi_header(&packet, 64).is_empty());
}

#[test]
fn ftdi_tail_partial_packet() {
    let input = decode_hex("000041");
    assert_eq!(strip_ftdi_header(&input, 64), decode_hex("41"));
}

#[test]
fn xon_xoff_strip_17_fixture() {
    let input = decode_hex("41421143");
    let mut f = XonXoffRxFilter::new(true);
    assert_eq!(f.filter(&input), decode_hex("414243"));
}

#[test]
fn xon_xoff_strip_19_fixture() {
    let input = decode_hex("41131143");
    let mut f = XonXoffRxFilter::new(true);
    assert_eq!(f.filter(&input), decode_hex("4143"));
}
