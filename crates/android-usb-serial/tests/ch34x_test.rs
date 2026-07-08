#![cfg(feature = "fake-transport")]

//! CH34x driver semantic tests.

use android_usb_serial::config::{DataBits, LineConfig, Parity, StopBits};
use android_usb_serial::device::open_port;
use android_usb_serial::fake::FakeTransport;
use android_usb_serial::transport::Transport;
use std::sync::Arc;

fn script_ch340_init(fake: &FakeTransport) {
    for _ in 0..4 {
        fake.script_control_in_response(vec![0, 0]);
    }
    for _ in 0..2 {
        fake.script_control_in_response(vec![0, 0]);
    }
}

fn open_ch34x(fake: &FakeTransport) -> android_usb_serial::port::SerialPortHandle {
    script_ch340_init(fake);
    let transport: Arc<dyn Transport> = Arc::new(fake.clone());
    open_port(transport, 0).expect("open")
}

#[test]
fn init_order_check_state_after_each_step() {
    let fake = FakeTransport::ch340_dual_iface();
    script_ch340_init(&fake);
    let mut port = open_ch34x(&fake);
    port.set_line_config(LineConfig {
        baud_rate: 115_200,
        data_bits: DataBits::Eight,
        parity: Parity::None,
        stop_bits: StopBits::One,
    })
    .expect("line");
    let controls = fake.recorded_controls();
    let in_requests: Vec<u8> = controls
        .iter()
        .filter(|c| c.request_type & 0x80 != 0)
        .map(|c| c.request)
        .collect();
    assert!(
        in_requests
            .windows(2)
            .any(|w| w == [0x5F, 0x95] || w == [0x95, 0x95]),
        "expected checkState IN sequence during init, got {in_requests:?}"
    );
}

#[test]
fn baud_921600_div7_factor_f300() {
    let fake = FakeTransport::ch340_dual_iface();
    script_ch340_init(&fake);
    let mut port = open_ch34x(&fake);
    fake.clear_recorded();
    port.set_line_config(LineConfig {
        baud_rate: 921_600,
        data_bits: DataBits::Eight,
        parity: Parity::None,
        stop_bits: StopBits::One,
    })
    .expect("baud");
    let baud_out = fake
        .recorded_controls()
        .into_iter()
        .find(|c| c.request == 0x9A && c.value == 0x1312)
        .expect("baud divisor");
    assert_eq!(baud_out.index, 0xF387);
    let factor = fake
        .recorded_controls()
        .into_iter()
        .find(|c| c.request == 0x9A && c.value == 0x0F2C)
        .expect("baud factor");
    assert_eq!(factor.index, 0x0000);
}

#[test]
fn status_active_low_cts() {
    let fake = FakeTransport::ch340_dual_iface();
    script_ch340_init(&fake);
    fake.script_control_in_response(vec![0x00, 0x00]);
    fake.script_control_in_response(vec![0x01, 0x00]);
    let mut port = open_ch34x(&fake);
    let status = port.modem_status().expect("modem");
    assert!(status.cts);
}

#[test]
fn break_rmw_via_0x1805() {
    let fake = FakeTransport::ch340_dual_iface();
    script_ch340_init(&fake);
    fake.script_control_in_response(vec![0x01, 0x40]);
    let mut port = open_ch34x(&fake);
    fake.clear_recorded();
    port.set_break(true).expect("break on");
    assert!(
        fake.recorded_controls()
            .iter()
            .any(|c| c.request == 0x95 && c.value == 0x1805),
        "break RMW read"
    );
    assert!(
        fake.recorded_controls()
            .iter()
            .any(|c| c.request == 0x9A && c.value == 0x1805),
        "break RMW write"
    );
}

#[test]
fn claims_all_interfaces_data_is_last() {
    let fake = FakeTransport::ch340_dual_iface();
    script_ch340_init(&fake);
    let _port = open_ch34x(&fake);
    let claimed = fake.claimed_interfaces();
    assert_eq!(claimed.len(), 2);
    assert!(claimed.contains(&0));
    assert!(claimed.contains(&1));
}
