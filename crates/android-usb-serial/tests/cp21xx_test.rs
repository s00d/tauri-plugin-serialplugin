#![cfg(feature = "fake-transport")]

//! CP21xx driver semantic tests.

use android_usb_serial::config::{DataBits, FlowControl, LineConfig, Parity, StopBits};
use android_usb_serial::device::open_port;
use android_usb_serial::fake::FakeTransport;
use android_usb_serial::probe::{DriverType, ProbeTable};
use android_usb_serial::transport::Transport;
use std::sync::Arc;

fn cp2105_fake() -> FakeTransport {
    FakeTransport::cp2105()
}

fn open_cp21xx(fake: &FakeTransport, port: usize) -> android_usb_serial::port::SerialPortHandle {
    let transport: Arc<dyn Transport> = Arc::new(fake.clone());
    open_port(transport, port).expect("open")
}

#[test]
fn ifc_enable_on_open_disable_on_close() {
    let fake = FakeTransport::cp2102();
    let mut port = open_cp21xx(&fake, 0);
    assert!(
        fake.recorded_controls()
            .iter()
            .any(|c| c.request == 0x00 && c.value == 1),
        "IFC_ENABLE on open"
    );
    port.close();
    assert!(
        fake.recorded_controls()
            .iter()
            .any(|c| c.request == 0x00 && c.value == 0),
        "IFC_DISABLE on close"
    );
}

#[test]
fn xon_xoff_set_chars_flow_xon_sequence() {
    let table = ProbeTable::default_table();
    assert_eq!(table.find(0x10C4, 0xEA60, &[]), DriverType::Cp21xx);

    let fake = FakeTransport::cp2102();
    let mut port = open_cp21xx(&fake, 0);
    fake.clear_recorded();
    port.set_flow_control(FlowControl::XonXoff)
        .expect("xon/xoff");
    let controls = fake.recorded_controls();
    assert!(
        controls.iter().any(|c| c.request == 0x19),
        "SET_CHARS expected"
    );
    assert!(
        controls.iter().any(|c| c.request == 0x13),
        "SET_FLOW expected"
    );
    assert!(
        controls.iter().any(|c| c.request == 0x09),
        "SET_XON expected"
    );
}

#[test]
fn restricted_port1_mark_parity_unsupported() {
    let fake = cp2105_fake();
    let mut port = open_cp21xx(&fake, 1);
    let err = port.set_line_config(LineConfig {
        baud_rate: 9600,
        data_bits: DataBits::Eight,
        parity: Parity::Mark,
        stop_bits: StopBits::One,
    });
    assert!(err.is_err());
}

#[test]
fn restricted_port1_stop2_unsupported() {
    let fake = cp2105_fake();
    let mut port = open_cp21xx(&fake, 1);
    let err = port.set_line_config(LineConfig {
        baud_rate: 9600,
        data_bits: DataBits::Eight,
        parity: Parity::None,
        stop_bits: StopBits::Two,
    });
    assert!(err.is_err());
}

#[test]
fn get_mdmsts_decodes_cts() {
    let fake = FakeTransport::cp2102();
    fake.script_control_in_response(vec![0x10]);
    let mut port = open_cp21xx(&fake, 0);
    let status = port.modem_status().expect("mdmsts");
    assert!(status.cts);
}
