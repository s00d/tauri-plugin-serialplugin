#![cfg(feature = "fake-transport")]

//! Replay golden fixtures against Rust drivers (Java byte-parity).

use android_usb_serial::config::{DataBits, FlowControl, LineConfig, Parity, PurgeKind, StopBits};
use android_usb_serial::drivers::create_driver;
use android_usb_serial::error::UsbSerialError;
use android_usb_serial::fake::{FakeTransport, RecordedBulkOut, RecordedControl};
use android_usb_serial::probe::DriverType;
use android_usb_serial::transport::{EndpointInfo, InterfaceInfo, Transport};
use base64::Engine;
use serde::Deserialize;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Deserialize)]
struct GoldenFixture {
    #[serde(default)]
    source: Option<String>,
    driver: String,
    scenario: String,
    #[serde(rename = "vendorId")]
    vendor_id: u16,
    #[serde(rename = "productId")]
    product_id: u16,
    #[serde(rename = "portIndex")]
    port_index: usize,
    #[serde(rename = "deviceStub")]
    device_stub: String,
    interfaces: Vec<GoldenInterface>,
    controls: Vec<GoldenControl>,
    #[serde(rename = "bulkOut", default)]
    bulk_out: Vec<GoldenBulkOut>,
    #[serde(rename = "expectError", default)]
    expect_error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GoldenInterface {
    id: u8,
    #[serde(rename = "classId")]
    class_id: u8,
    subclass: u8,
    protocol: u8,
    #[serde(rename = "bulkIn")]
    bulk_in: Option<u8>,
    #[serde(rename = "bulkOut")]
    bulk_out: Option<u8>,
    #[serde(rename = "interruptIn")]
    interrupt_in: Option<u8>,
    #[serde(rename = "maxPacketSize", default = "default_mps")]
    max_packet_size: u16,
}

fn default_mps() -> u16 {
    64
}

fn deserialize_u16_signed<'de, D>(deserializer: D) -> Result<u16, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v = i32::deserialize(deserializer)?;
    Ok(v as u16)
}

#[derive(Debug, Deserialize)]
struct GoldenControl {
    #[serde(rename = "requestType")]
    request_type: u8,
    request: u8,
    #[serde(deserialize_with = "deserialize_u16_signed")]
    value: u16,
    #[serde(deserialize_with = "deserialize_u16_signed")]
    index: u16,
    data: String,
}

#[derive(Debug, Deserialize)]
struct GoldenBulkOut {
    endpoint: u8,
    data: String,
}

#[derive(Debug, Deserialize)]
struct RxFilterFixture {
    filter: String,
    input: String,
    output: String,
}

#[derive(Debug, Deserialize)]
struct ProbeTableEntry {
    #[serde(rename = "vendorId")]
    vendor_id: u16,
    #[serde(rename = "productId")]
    product_id: u16,
    driver: String,
    #[serde(rename = "portCount", default)]
    port_count: usize,
}

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn driver_type(name: &str) -> DriverType {
    match name {
        "cdc_acm" => DriverType::CdcAcm,
        "ftdi" => DriverType::Ftdi,
        "cp21xx" => DriverType::Cp21xx,
        "ch34x" => DriverType::Ch34x,
        "prolific" => DriverType::Prolific,
        "gsm_modem" => DriverType::GsmModem,
        "chrome_ccd" => DriverType::ChromeCcd,
        other => panic!("unknown driver {other}"),
    }
}

fn line_115200() -> LineConfig {
    LineConfig {
        baud_rate: 115_200,
        data_bits: DataBits::Eight,
        parity: Parity::None,
        stop_bits: StopBits::One,
    }
}

fn prolific_status_packet() -> Vec<u8> {
    let mut status = vec![0u8; 10];
    status[0] = 0xA1;
    status
}

fn setup_fake(fixture: &GoldenFixture) -> FakeTransport {
    let fake = match fixture.device_stub.as_str() {
        "ftdi_ft232r" => FakeTransport::ftdi_ft232r(),
        "ftdi_ft2232h" => FakeTransport::ftdi_ft2232(),
        "cp2102" => FakeTransport::cp2102(),
        "cp2105" => FakeTransport::cp2105(),
        "ch340" => {
            let t = FakeTransport::ch340_dual_iface();
            for _ in 0..8 {
                t.script_control_in_response(vec![0, 0]);
            }
            t
        }
        "pl2303_hx" => {
            let t = FakeTransport::pl2303_hx();
            t.script_control_in_response(vec![0]);
            let status = prolific_status_packet();
            t.push_interrupt_in(&status);
            t.push_interrupt_in(&status);
            t
        }
        "pl2303_hxn" => {
            let t = FakeTransport::pl2303_hxn();
            let status = prolific_status_packet();
            t.push_interrupt_in(&status);
            t.push_interrupt_in(&status);
            t
        }
        "pl2303_01" => {
            let t = FakeTransport::pl2303_type01();
            t.script_control_in_response(vec![0]);
            let status = prolific_status_packet();
            t.push_interrupt_in(&status);
            t.push_interrupt_in(&status);
            t
        }
        "pl2303_ta" => {
            let t = FakeTransport::pl2303_ta();
            t.script_control_in_response(vec![0]);
            let status = prolific_status_packet();
            t.push_interrupt_in(&status);
            t.push_interrupt_in(&status);
            t
        }
        "cdc_single" => FakeTransport::cdc_single_iface(),
        "cdc_iad" => FakeTransport::cdc_iad(),
        "cdc_multi" => FakeTransport::cdc_multi(),
        "gsm_fibocom" => FakeTransport::gsm_modem(),
        "chrome_ccd" | "cr50" => FakeTransport::chrome_ccd_3port(),
        other => panic!("unknown device stub {other}"),
    };
    fake.set_vendor_product(fixture.vendor_id, fixture.product_id);
    let ifaces: Vec<InterfaceInfo> = fixture
        .interfaces
        .iter()
        .map(|i| InterfaceInfo {
            id: i.id,
            class: i.class_id,
            subclass: i.subclass,
            protocol: i.protocol,
        })
        .collect();
    fake.set_interfaces(ifaces);
    let mut layout: Vec<(u8, Vec<EndpointInfo>)> = Vec::new();
    for i in &fixture.interfaces {
        let mut eps = Vec::new();
        if let Some(addr) = i.interrupt_in {
            eps.push(EndpointInfo {
                address: addr,
                attributes: 3,
                max_packet_size: i.max_packet_size,
                interval: 1,
            });
        }
        if let Some(addr) = i.bulk_in {
            eps.push(EndpointInfo {
                address: addr,
                attributes: 2,
                max_packet_size: i.max_packet_size,
                interval: 0,
            });
        }
        if let Some(addr) = i.bulk_out {
            eps.push(EndpointInfo {
                address: addr,
                attributes: 2,
                max_packet_size: i.max_packet_size,
                interval: 0,
            });
        }
        layout.push((i.id, eps));
    }
    fake.configure_endpoints(&layout);
    fake
}

fn scenario_suffix(fixture: &GoldenFixture) -> String {
    let stub_prefix = format!("{}_", fixture.device_stub);
    let rest = fixture
        .scenario
        .strip_prefix(&stub_prefix)
        .unwrap_or(&fixture.scenario);
    let port_prefix = format!("port{}_", fixture.port_index);
    rest.strip_prefix(&port_prefix).unwrap_or(rest).to_string()
}

fn parse_line_suffix(name: &str) -> LineConfig {
    let (db, rest) = name.split_at(1);
    let data_bits = match db {
        "5" => DataBits::Five,
        "7" => DataBits::Seven,
        "8" => DataBits::Eight,
        other => panic!("unsupported data bits {other} in set_line_{name}"),
    };
    let parity_ch = rest.chars().next().expect("parity");
    let parity = match parity_ch {
        'N' => Parity::None,
        'E' => Parity::Even,
        'O' => Parity::Odd,
        'M' => Parity::Mark,
        'S' => Parity::Space,
        other => panic!("unsupported parity {other}"),
    };
    let sb_part = &rest[1..];
    let stop_bits = if sb_part == "1" {
        StopBits::One
    } else if sb_part == "2" {
        StopBits::Two
    } else if sb_part == "1_5" {
        StopBits::OnePointFive
    } else {
        panic!("unsupported stop bits {sb_part}");
    };
    LineConfig {
        baud_rate: 115_200,
        data_bits,
        parity,
        stop_bits,
    }
}

fn is_limited_driver(driver: &str) -> bool {
    matches!(driver, "gsm_modem" | "chrome_ccd")
}

fn replay_scenario(
    driver: &mut dyn android_usb_serial::drivers::Driver,
    driver_name: &str,
    suffix: &str,
) -> Result<(), UsbSerialError> {
    match suffix {
        "open_default" => {
            if !is_limited_driver(driver_name) {
                driver.set_line_config(line_115200())?;
                let _ = driver.set_dtr(true);
                let _ = driver.set_rts(true);
            }
            Ok(())
        }
        "open_line_config" => {
            driver.set_line_config(line_115200())?;
            let _ = driver.set_dtr(true);
            let _ = driver.set_rts(true);
            Ok(())
        }
        s if s.starts_with("set_baud_") && !s.contains("error") => {
            let baud: u32 = s["set_baud_".len()..].parse().expect("baud");
            let mut cfg = line_115200();
            cfg.baud_rate = baud;
            driver.set_line_config(cfg)
        }
        s if s.starts_with("set_baud_error_") => {
            let baud: u32 = s["set_baud_error_".len()..].parse().expect("baud");
            let mut cfg = line_115200();
            cfg.baud_rate = baud;
            driver.set_line_config(cfg)
        }
        s if s.starts_with("set_line_") => {
            let name = &s["set_line_".len()..];
            driver.set_line_config(parse_line_suffix(name))
        }
        "flow_None" => {
            driver.set_line_config(line_115200()).ok();
            driver.set_flow_control(FlowControl::None)
        }
        "flow_RtsCts" => {
            driver.set_line_config(line_115200()).ok();
            driver.set_flow_control(FlowControl::RtsCts)
        }
        "flow_DtrDsr" => {
            driver.set_line_config(line_115200()).ok();
            driver.set_flow_control(FlowControl::DtrDsr)
        }
        "flow_XonXoff" => {
            driver.set_line_config(line_115200()).ok();
            driver.set_flow_control(FlowControl::XonXoff)
        }
        "flow_XonXoffInline" => {
            driver.set_line_config(line_115200()).ok();
            driver.set_flow_control(FlowControl::XonXoffInline)
        }
        "dtr_on" => driver.set_dtr(true),
        "dtr_off" => driver.set_dtr(false),
        "rts_on" => driver.set_rts(true),
        "rts_off" => driver.set_rts(false),
        "break_on" => driver.set_break(true),
        "break_off" => driver.set_break(false),
        "purge_rx" => driver.purge(PurgeKind::Rx),
        "purge_tx" => driver.purge(PurgeKind::Tx),
        "purge_both" => driver.purge(PurgeKind::Both),
        "modem_status" => {
            driver.set_line_config(line_115200())?;
            for _ in 0..4 {
                let _ = driver.modem_status();
            }
            Ok(())
        }
        "close" => {
            if !is_limited_driver(driver_name) {
                let _ = driver.set_line_config(line_115200());
            }
            Ok(())
        }
        other => panic!("unsupported scenario suffix {other}"),
    }
}

fn hex(data: &[u8]) -> String {
    data.iter().map(|b| format!("{b:02x}")).collect()
}

fn decode_b64(s: &str) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode(s)
        .unwrap_or_default()
}

fn format_control_diff(path: &Path, got: &[RecordedControl], want: &[GoldenControl]) -> String {
    let mut out = format!(
        "control mismatch for {}: got {} want {}\n",
        path.display(),
        got.len(),
        want.len()
    );
    let n = got.len().max(want.len());
    for i in 0..n {
        match (got.get(i), want.get(i)) {
            (Some(g), Some(w)) => {
                let want_data = decode_b64(&w.data);
                if g.request_type != w.request_type
                    || g.request != w.request
                    || g.value != w.value
                    || g.index != w.index
                    || g.data != want_data
                {
                    let _ = writeln!(
                        out,
                        "#{} requestType={}/{} request={}/{} value={:#06x}/{:#06x} index={}/{} data=hex:{} expected=hex:{}",
                        i,
                        g.request_type,
                        w.request_type,
                        g.request,
                        w.request,
                        g.value,
                        w.value,
                        g.index,
                        w.index,
                        hex(&g.data),
                        hex(&want_data),
                    );
                }
            }
            (Some(g), None) => {
                let _ =
                    writeln!(
                    out,
                    "#{} extra got requestType={} request={} value={:#06x} index={} data=hex:{}",
                    i, g.request_type, g.request, g.value, g.index, hex(&g.data),
                );
            }
            (None, Some(w)) => {
                let want_data = decode_b64(&w.data);
                let _ = writeln!(
                    out,
                    "#{} missing expected requestType={} request={} value={:#06x} index={} data=hex:{}",
                    i, w.request_type, w.request, w.value, w.index, hex(&want_data),
                );
            }
            (None, None) => {}
        }
    }
    out
}

fn controls_match(got: &[RecordedControl], want: &[GoldenControl]) -> bool {
    got.len() == want.len()
        && got.iter().zip(want.iter()).all(|(g, w)| {
            g.request_type == w.request_type
                && g.request == w.request
                && g.value == w.value
                && g.index == w.index
                && g.data == decode_b64(&w.data)
        })
}

fn assert_controls(path: &Path, got: &[RecordedControl], want: &[GoldenControl]) {
    if !controls_match(got, want) {
        panic!("{}", format_control_diff(path, got, want));
    }
}

fn format_bulk_diff(path: &Path, got: &[RecordedBulkOut], want: &[GoldenBulkOut]) -> String {
    let mut out = format!(
        "bulkOut mismatch for {}: got {} want {}\n",
        path.display(),
        got.len(),
        want.len()
    );
    let n = got.len().max(want.len());
    for i in 0..n {
        match (got.get(i), want.get(i)) {
            (Some(g), Some(w)) => {
                let want_data = decode_b64(&w.data);
                if g.endpoint != w.endpoint || g.data != want_data {
                    let _ = writeln!(
                        out,
                        "#{} endpoint={}/{} data=hex:{} expected=hex:{}",
                        i,
                        g.endpoint,
                        w.endpoint,
                        hex(&g.data),
                        hex(&want_data),
                    );
                }
            }
            (Some(g), None) => {
                let _ = writeln!(
                    out,
                    "#{} extra bulkOut endpoint={} data=hex:{}",
                    i,
                    g.endpoint,
                    hex(&g.data),
                );
            }
            (None, Some(w)) => {
                let want_data = decode_b64(&w.data);
                let _ = writeln!(
                    out,
                    "#{} missing bulkOut endpoint={} data=hex:{}",
                    i,
                    w.endpoint,
                    hex(&want_data),
                );
            }
            (None, None) => {}
        }
    }
    out
}

fn assert_bulk_out(path: &Path, got: &[RecordedBulkOut], want: &[GoldenBulkOut]) {
    if got.len() != want.len()
        || got
            .iter()
            .zip(want.iter())
            .any(|(g, w)| g.endpoint != w.endpoint || g.data != decode_b64(&w.data))
    {
        panic!("{}", format_bulk_diff(path, got, want));
    }
}

fn decode_hex(s: &str) -> Vec<u8> {
    let s = s.trim().trim_start_matches("0x");
    if s.is_empty() {
        return Vec::new();
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
        .collect()
}

fn error_class(err: &UsbSerialError) -> &'static str {
    match err {
        UsbSerialError::Unsupported(_) => "UnsupportedOperationException",
        UsbSerialError::ProbeFailed(msg) | UsbSerialError::Io(msg)
            if msg.to_lowercase().contains("invalid") =>
        {
            "IllegalArgumentException"
        }
        UsbSerialError::ProbeFailed(_) | UsbSerialError::Io(_) => "Exception",
        _ => "Exception",
    }
}

fn collect_driver_fixtures() -> Vec<PathBuf> {
    let root = fixture_dir();
    let mut paths = Vec::new();
    for entry in fs::read_dir(&root).expect("read fixtures dir") {
        let driver_dir = entry.expect("entry").path();
        if !driver_dir.is_dir() {
            continue;
        }
        let name = driver_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        if name == "rx_filter" || name == "probe" {
            continue;
        }
        for file in fs::read_dir(&driver_dir).expect("driver dir") {
            let p = file.expect("file").path();
            if p.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let text = fs::read_to_string(&p).unwrap_or_default();
            if text.contains("\"source\":\"java\"") || text.contains("\"source\": \"java\"") {
                paths.push(p);
            }
        }
    }
    paths.sort();
    paths
}

fn replay_fixture(path: &Path) {
    let text = fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let fixture: GoldenFixture =
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    assert_eq!(
        fixture.source.as_deref(),
        Some("java"),
        "expected Java fixture {}",
        path.display()
    );
    let fake = setup_fake(&fixture);
    let transport: Arc<dyn Transport> = Arc::new(fake.clone());
    let mut driver = create_driver(driver_type(&fixture.driver), fixture.port_index);
    driver.open(&transport).expect("open");
    let suffix = scenario_suffix(&fixture);
    let result = replay_scenario(driver.as_mut(), &fixture.driver, &suffix);
    let _ = driver.close();
    if let Some(expected) = &fixture.expect_error {
        match result {
            Err(e) => assert_eq!(
                error_class(&e),
                expected.as_str(),
                "{}: expected {expected}, got {e:?}",
                path.display()
            ),
            Ok(()) => panic!(
                "{}: expected error {expected} but scenario succeeded",
                path.display()
            ),
        }
    } else {
        result.unwrap_or_else(|e| panic!("{}: scenario {suffix}: {e:?}", path.display()));
    }
    assert_controls(path, &fake.recorded_controls(), &fixture.controls);
    assert_bulk_out(path, &fake.recorded_bulk_out(), &fixture.bulk_out);
}

#[test]
fn golden_parity_all_java_fixtures() {
    let paths = collect_driver_fixtures();
    assert!(
        paths.len() >= 250,
        "expected >=250 Java fixtures, got {}",
        paths.len()
    );
    for path in paths {
        replay_fixture(&path);
    }
}

#[test]
fn golden_parity_rx_filter_fixtures() {
    let dir = fixture_dir().join("rx_filter");
    if !dir.exists() {
        eprintln!("skip: no rx_filter fixtures at {}", dir.display());
        return;
    }
    use android_usb_serial::rx_filter::{strip_ftdi_header, RxFilter, XonXoffRxFilter};
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
            other => panic!("unknown rx filter {other}"),
        };
        assert_eq!(
            got,
            want,
            "rx_filter {}: input={} want={} got={}",
            path.display(),
            fx.input,
            fx.output,
            hex(&got),
        );
    }
}

#[test]
fn golden_parity_probe_table() {
    let path = fixture_dir().join("probe/probe_table.json");
    if !path.exists() {
        eprintln!("skip: no probe_table at {}", path.display());
        return;
    }
    use android_usb_serial::probe::{DriverType, ProbeTable};
    let text = fs::read_to_string(&path).unwrap();
    let entries: Vec<ProbeTableEntry> = serde_json::from_str(&text).unwrap();
    assert!(!entries.is_empty(), "probe_table empty");
    let table = ProbeTable::default_table();
    for entry in entries {
        let driver = table.find(entry.vendor_id, entry.product_id, &[]);
        let expected = match entry.driver.as_str() {
            "cdc_acm" => DriverType::CdcAcm,
            "ftdi" => DriverType::Ftdi,
            "cp21xx" => DriverType::Cp21xx,
            "ch34x" => DriverType::Ch34x,
            "prolific" => DriverType::Prolific,
            "gsm_modem" => DriverType::GsmModem,
            "chrome_ccd" => DriverType::ChromeCcd,
            other => panic!("unknown driver {other}"),
        };
        assert_eq!(
            driver, expected,
            "vid={:#06x} pid={:#06x}",
            entry.vendor_id, entry.product_id,
        );
        if entry.port_count > 0 {
            assert_eq!(
                table.port_count_product(entry.vendor_id, entry.product_id, driver, &[]),
                entry.port_count,
                "vid={:#06x} pid={:#06x}",
                entry.vendor_id,
                entry.product_id,
            );
        }
    }
}
