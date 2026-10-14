use android_usb_serial::config::{FlowControl, LineConfig, PurgeKind};
use android_usb_serial::error::UsbSerialError;
use android_usb_serial::fake::FakeTransport;
use android_usb_serial::open_port;
use android_usb_serial::serialport_compat::SerialPortAdapter;
use serialport::SerialPort;
use std::io::{Read, Write};
use std::sync::Arc;

#[test]
fn stale_clone_write_after_detach_returns_error() {
    let handle = open_port(Arc::new(FakeTransport::cdc_iad()), 0).unwrap();
    let adapter =
        SerialPortAdapter::new(handle, "usb#0", LineConfig::default(), FlowControl::None).unwrap();
    adapter.start_reader().unwrap();
    let mut pending_writer = adapter.clone();
    adapter.shutdown();
    assert!(pending_writer.write(b"1").is_err());
    assert!(pending_writer.read(&mut [0; 8]).is_err());
    assert!(pending_writer.write_data_terminal_ready(true).is_err());
    adapter.shutdown();
}

#[test]
fn closed_handle_rejects_io_and_controls() {
    let mut handle = open_port(Arc::new(FakeTransport::cdc_iad()), 0).unwrap();
    handle.close();
    assert_eq!(handle.write(b"1"), Err(UsbSerialError::Disconnected));
    assert_eq!(handle.read(&mut [0; 8]), Err(UsbSerialError::Disconnected));
    assert_eq!(
        handle.try_read(&mut [0; 8]),
        Err(UsbSerialError::Disconnected)
    );
    assert_eq!(handle.start_reader(), Err(UsbSerialError::Disconnected));
    assert_eq!(
        handle.set_line_config(LineConfig::default()),
        Err(UsbSerialError::Disconnected)
    );
    assert_eq!(
        handle.set_flow_control(FlowControl::None),
        Err(UsbSerialError::Disconnected)
    );
    assert_eq!(handle.set_dtr(true), Err(UsbSerialError::Disconnected));
    assert_eq!(handle.set_rts(true), Err(UsbSerialError::Disconnected));
    assert_eq!(handle.set_break(true), Err(UsbSerialError::Disconnected));
    assert_eq!(
        handle.purge(PurgeKind::Both),
        Err(UsbSerialError::Disconnected)
    );
    assert_eq!(
        handle.clear(PurgeKind::Both),
        Err(UsbSerialError::Disconnected)
    );
    assert!(matches!(
        handle.modem_status(),
        Err(UsbSerialError::Disconnected)
    ));
    handle.close();
}

#[test]
fn fresh_handle_can_write_after_previous_session_closes() {
    let fake = FakeTransport::cdc_iad();
    let mut old = open_port(Arc::new(fake.clone()), 0).unwrap();
    old.close();
    let mut fresh = open_port(Arc::new(fake), 0).unwrap();
    assert_eq!(fresh.write(b"1").unwrap(), 1);
}
