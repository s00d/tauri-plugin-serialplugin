//! Record golden USB control-transfer fixtures from Rust drivers (regen tool).
//!
//! Run: `cargo run -p android-usb-serial --features fake-transport --bin golden_record`

use android_usb_serial::config::{DataBits, FlowControl, LineConfig, Parity, PurgeKind, StopBits};
use android_usb_serial::drivers::create_driver;
use android_usb_serial::fake::{FakeTransport, RecordedControl};
use android_usb_serial::probe::DriverType;
use android_usb_serial::transport::{EndpointInfo, InterfaceInfo, Transport};
use base64::Engine;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Clone)]
struct Preset {
    driver: &'static str,
    driver_type: DriverType,
    vendor_id: u16,
    product_id: u16,
    port_index: usize,
    interfaces: Vec<GoldenInterface>,
    build_fake: fn() -> FakeTransport,
}

#[derive(Serialize)]
struct GoldenFixture {
    driver: String,
    scenario: String,
    #[serde(rename = "vendorId")]
    vendor_id: u16,
    #[serde(rename = "productId")]
    product_id: u16,
    #[serde(rename = "portIndex")]
    port_index: usize,
    interfaces: Vec<GoldenInterface>,
    controls: Vec<GoldenControl>,
}

#[derive(Serialize, Clone)]
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
    #[serde(rename = "maxPacketSize")]
    max_packet_size: u16,
}

#[derive(Serialize)]
struct GoldenControl {
    #[serde(rename = "requestType")]
    request_type: u8,
    request: u8,
    value: u16,
    index: u16,
    data: String,
}

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn encode_controls(controls: &[RecordedControl]) -> Vec<GoldenControl> {
    controls
        .iter()
        .map(|c| GoldenControl {
            request_type: c.request_type,
            request: c.request,
            value: c.value,
            index: c.index,
            data: base64::engine::general_purpose::STANDARD.encode(&c.data),
        })
        .collect()
}

fn iface_layout(
    id: u8,
    class_id: u8,
    bulk_in: u8,
    bulk_out: u8,
    interrupt_in: Option<u8>,
) -> (InterfaceInfo, GoldenInterface, (u8, Vec<EndpointInfo>)) {
    let info = InterfaceInfo {
        id,
        class: class_id,
        subclass: 0,
        protocol: 0,
    };
    let golden = GoldenInterface {
        id,
        class_id,
        subclass: 0,
        protocol: 0,
        bulk_in: Some(bulk_in),
        bulk_out: Some(bulk_out),
        interrupt_in,
        max_packet_size: 64,
    };
    let mut eps = vec![
        EndpointInfo {
            address: bulk_in,
            attributes: 2,
            max_packet_size: 64,
            interval: 0,
        },
        EndpointInfo {
            address: bulk_out,
            attributes: 2,
            max_packet_size: 64,
            interval: 0,
        },
    ];
    if let Some(int_ep) = interrupt_in {
        eps.insert(
            0,
            EndpointInfo {
                address: int_ep,
                attributes: 3,
                max_packet_size: 64,
                interval: 1,
            },
        );
    }
    (info, golden, (id, eps))
}

fn build_ftdi() -> FakeTransport {
    let fake = FakeTransport::ftdi_ft232r();
    let (i0, _, e0) = iface_layout(0, 255, 0x81, 0x02, None);
    fake.set_interfaces(vec![i0]);
    fake.configure_endpoints(&[e0]);
    fake
}

fn build_cp21xx() -> FakeTransport {
    let fake = FakeTransport::cp2102();
    let (i0, _, e0) = iface_layout(0, 255, 0x81, 0x02, None);
    fake.set_interfaces(vec![i0]);
    fake.configure_endpoints(&[e0]);
    fake
}

fn build_ch34x() -> FakeTransport {
    let fake = FakeTransport::ch340_dual_iface();
    for _ in 0..4 {
        fake.script_control_in_response(vec![0, 0]);
    }
    let (i0, _, _) = iface_layout(0, 255, 0x81, 0x02, None);
    let (_, _, e1) = iface_layout(1, 255, 0x82, 0x03, None);
    fake.set_interfaces(vec![
        i0,
        InterfaceInfo {
            id: 1,
            class: 255,
            subclass: 0,
            protocol: 0,
        },
    ]);
    fake.configure_endpoints(&[e1]);
    fake
}

fn build_prolific() -> FakeTransport {
    let fake = FakeTransport::pl2303_hx();
    fake.script_control_in_response(vec![0]);
    let (i0, _, e0) = iface_layout(0, 255, 0x83, 0x02, Some(0x81));
    fake.set_interfaces(vec![i0]);
    fake.configure_endpoints(&[e0]);
    fake
}

fn build_cdc() -> FakeTransport {
    let fake = FakeTransport::cdc_single_iface();
    fake.set_vendor_product(0x2341, 0x0043);
    let i0 = InterfaceInfo {
        id: 0,
        class: 2,
        subclass: 2,
        protocol: 0,
    };
    let i1 = InterfaceInfo {
        id: 1,
        class: 10,
        subclass: 0,
        protocol: 0,
    };
    let (_, _, e0) = iface_layout(0, 2, 0x81, 0x02, None);
    let (_, _, e1) = iface_layout(1, 10, 0x83, 0x04, None);
    fake.set_interfaces(vec![i0, i1]);
    fake.configure_endpoints(&[e0, e1]);
    fake
}

fn build_gsm() -> FakeTransport {
    let fake = FakeTransport::cdc_single_iface();
    fake.set_vendor_product(0x1782, 0x4D10);
    let (i0, _, e0) = iface_layout(0, 255, 0x81, 0x02, None);
    fake.set_interfaces(vec![i0]);
    fake.configure_endpoints(&[e0]);
    fake
}

fn build_ccd() -> FakeTransport {
    let fake = FakeTransport::cdc_single_iface();
    fake.set_vendor_product(0x18D1, 0x5014);
    let mut ifaces = Vec::new();
    let mut eps = Vec::new();
    for n in 0..3u8 {
        let (i, _, e) = iface_layout(n, 255, 0x81 + n * 2, 0x02 + n * 2, None);
        ifaces.push(i);
        eps.push(e);
    }
    fake.set_interfaces(ifaces);
    fake.configure_endpoints(&eps);
    fake
}

fn presets() -> Vec<Preset> {
    let (_, g_ftdi, _) = iface_layout(0, 255, 0x81, 0x02, None);
    let (_, g_cp, _) = iface_layout(0, 255, 0x81, 0x02, None);
    let (_, g_ch, _) = iface_layout(1, 255, 0x82, 0x03, None);
    let (_, g_pl, _) = iface_layout(0, 255, 0x83, 0x02, Some(0x81));
    let g_cdc0 = GoldenInterface {
        id: 0,
        class_id: 2,
        subclass: 2,
        protocol: 0,
        bulk_in: Some(0x81),
        bulk_out: Some(0x02),
        interrupt_in: None,
        max_packet_size: 64,
    };
    let (_, g_cdc1, _) = iface_layout(1, 10, 0x83, 0x04, None);
    let (_, g_gsm, _) = iface_layout(0, 255, 0x81, 0x02, None);
    let mut ccd_golden = Vec::new();
    for n in 0..3u8 {
        let (_, g, _) = iface_layout(n, 255, 0x81 + n * 2, 0x02 + n * 2, None);
        ccd_golden.push(g);
    }

    vec![
        Preset {
            driver: "ftdi",
            driver_type: DriverType::Ftdi,
            vendor_id: 0x0403,
            product_id: 0x6001,
            port_index: 0,
            interfaces: vec![g_ftdi],
            build_fake: build_ftdi,
        },
        Preset {
            driver: "cp21xx",
            driver_type: DriverType::Cp21xx,
            vendor_id: 0x10C4,
            product_id: 0xEA60,
            port_index: 0,
            interfaces: vec![g_cp],
            build_fake: build_cp21xx,
        },
        Preset {
            driver: "ch34x",
            driver_type: DriverType::Ch34x,
            vendor_id: 0x1A86,
            product_id: 0x7523,
            port_index: 0,
            interfaces: vec![g_ch],
            build_fake: build_ch34x,
        },
        Preset {
            driver: "prolific",
            driver_type: DriverType::Prolific,
            vendor_id: 0x067B,
            product_id: 0x2303,
            port_index: 0,
            interfaces: vec![g_pl],
            build_fake: build_prolific,
        },
        Preset {
            driver: "cdc_acm",
            driver_type: DriverType::CdcAcm,
            vendor_id: 0x2341,
            product_id: 0x0043,
            port_index: 0,
            interfaces: vec![g_cdc0, g_cdc1],
            build_fake: build_cdc,
        },
        Preset {
            driver: "gsm_modem",
            driver_type: DriverType::GsmModem,
            vendor_id: 0x1782,
            product_id: 0x4D10,
            port_index: 0,
            interfaces: vec![g_gsm],
            build_fake: build_gsm,
        },
        Preset {
            driver: "chrome_ccd",
            driver_type: DriverType::ChromeCcd,
            vendor_id: 0x18D1,
            product_id: 0x5014,
            port_index: 0,
            interfaces: ccd_golden,
            build_fake: build_ccd,
        },
    ]
}

fn record_scenario(
    preset: &Preset,
    scenario: &str,
    run: impl FnOnce(&mut dyn android_usb_serial::drivers::Driver, &Preset),
) {
    let fake = (preset.build_fake)();
    let transport: Arc<dyn Transport> = Arc::new(fake.clone());
    let mut driver = create_driver(preset.driver_type, preset.port_index);
    driver.open(&transport).expect("open");
    run(driver.as_mut(), preset);
    driver.close().expect("close");

    let fixture = GoldenFixture {
        driver: preset.driver.to_string(),
        scenario: scenario.to_string(),
        vendor_id: preset.vendor_id,
        product_id: preset.product_id,
        port_index: preset.port_index,
        interfaces: preset.interfaces.clone(),
        controls: encode_controls(&fake.recorded_controls()),
    };

    let dir = fixture_dir().join(preset.driver);
    fs::create_dir_all(&dir).expect("mkdir");
    let path = dir.join(format!("{scenario}.json"));
    if path.exists() {
        let existing = fs::read_to_string(&path).unwrap_or_default();
        if existing.contains("\"source\":\"java\"") || existing.contains("\"source\": \"java\"") {
            eprintln!("skip (java): {}", path.display());
            return;
        }
    }
    let json = serde_json::to_string_pretty(&fixture).expect("json");
    fs::write(&path, json).expect("write");
    eprintln!("wrote {}", path.display());
}

fn line_115200() -> LineConfig {
    LineConfig {
        baud_rate: 115_200,
        data_bits: DataBits::Eight,
        parity: Parity::None,
        stop_bits: StopBits::One,
    }
}

fn supported_flows(driver: &str) -> Vec<FlowControl> {
    match driver {
        "ftdi" => vec![
            FlowControl::None,
            FlowControl::RtsCts,
            FlowControl::DtrDsr,
            FlowControl::XonXoffInline,
        ],
        "cp21xx" => vec![
            FlowControl::None,
            FlowControl::RtsCts,
            FlowControl::DtrDsr,
            FlowControl::XonXoff,
        ],
        "prolific" => vec![
            FlowControl::None,
            FlowControl::RtsCts,
            FlowControl::XonXoffInline,
        ],
        _ => vec![FlowControl::None, FlowControl::RtsCts, FlowControl::DtrDsr],
    }
}

fn main() {
    let baud_rates = [
        300, 1200, 9600, 19200, 38400, 57600, 115_200, 230_400, 460_800, 921_600,
    ];

    for preset in presets() {
        record_scenario(&preset, "open_default", |d, p| {
            if p.driver != "gsm_modem" && p.driver != "chrome_ccd" {
                d.set_line_config(line_115200()).ok();
            }
            let _ = d.set_dtr(true);
            let _ = d.set_rts(true);
        });

        if preset.driver == "cdc_acm" {
            record_scenario(&preset, "open_line_config", |d, _| {
                d.set_line_config(line_115200()).expect("line");
                let _ = d.set_dtr(true);
                let _ = d.set_rts(true);
            });
        }

        if !matches!(preset.driver, "gsm_modem" | "chrome_ccd") {
            for &baud in &baud_rates {
                let name = format!("set_baud_{baud}");
                record_scenario(&preset, &name, |d, _| {
                    let mut cfg = line_115200();
                    cfg.baud_rate = baud;
                    d.set_line_config(cfg).ok();
                });
            }

            for flow in supported_flows(preset.driver) {
                let name = format!("flow_{flow:?}");
                record_scenario(&preset, &name, |d, _| {
                    d.set_line_config(line_115200()).ok();
                    d.set_flow_control(flow).ok();
                });
            }

            record_scenario(&preset, "dtr_on", |d, _| {
                d.set_dtr(true).ok();
            });
            record_scenario(&preset, "dtr_off", |d, _| {
                d.set_dtr(false).ok();
            });
            record_scenario(&preset, "rts_on", |d, _| {
                d.set_rts(true).ok();
            });
            record_scenario(&preset, "rts_off", |d, _| {
                d.set_rts(false).ok();
            });
            record_scenario(&preset, "break_on", |d, _| {
                d.set_break(true).ok();
            });
            record_scenario(&preset, "break_off", |d, _| {
                d.set_break(false).ok();
            });
            record_scenario(&preset, "purge_rx", |d, _| {
                d.purge(PurgeKind::Rx).ok();
            });
            record_scenario(&preset, "purge_tx", |d, _| {
                d.purge(PurgeKind::Tx).ok();
            });
            record_scenario(&preset, "purge_both", |d, _| {
                d.purge(PurgeKind::Both).ok();
            });
            record_scenario(&preset, "modem_status", |d, _| {
                let _ = d.modem_status();
            });
        }

        record_scenario(&preset, "close", |d, p| {
            if p.driver != "gsm_modem" && p.driver != "chrome_ccd" {
                d.set_line_config(line_115200()).ok();
            }
        });
    }
}
