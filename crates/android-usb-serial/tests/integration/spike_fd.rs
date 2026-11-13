//! fd → dup → open path on real hardware (Android only, ignored by default).

/// Real-device spike: `dup → from_fd → descriptor → claim → echo`.
/// Run on Android hardware:
/// `cargo test -p android-usb-serial --features fake-transport spike_fd_hardware -- --ignored`
#[cfg(all(target_os = "android", feature = "fake-transport"))]
#[test]
#[ignore = "requires USB serial device; pass fd via ANDROID_USB_SPIKE_FD"]
fn spike_fd_hardware() {
    use android_usb_serial::config::{DataBits, LineConfig, Parity, StopBits};
    use android_usb_serial::device::open_port;
    use android_usb_serial::from_raw_fd;
    use android_usb_serial::probe::ProbeTable;
    use android_usb_serial::transport::Transport;
    use android_usb_serial::NusbTransport;
    use std::env;
    use std::os::fd::RawFd;
    use std::sync::Arc;

    let fd: RawFd = env::var("ANDROID_USB_SPIKE_FD")
        .expect("set ANDROID_USB_SPIKE_FD to open UsbDeviceConnection fd")
        .parse()
        .expect("invalid fd");
    let device = from_raw_fd(fd).expect("from_fd");
    let transport = NusbTransport::from_device(device).expect("transport");
    let desc = transport.raw_device_descriptor();
    assert_eq!(desc[0], 18);
    assert!(!transport.raw_descriptors().is_empty());

    let vid = u16::from_le_bytes([desc[8], desc[9]]);
    let pid = u16::from_le_bytes([desc[10], desc[11]]);
    let table = ProbeTable::default_table();
    let ifaces = transport.interfaces();
    let _driver = table.find(vid, pid, &ifaces);

    let shared: Arc<dyn Transport> = Arc::new(transport);
    let mut port = open_port(shared, 0).expect("open_port");
    if port
        .set_line_config(LineConfig {
            baud_rate: 115_200,
            data_bits: DataBits::Eight,
            parity: Parity::None,
            stop_bits: StopBits::One,
        })
        .is_ok()
    {
        let _ = port.set_dtr(true);
        let _ = port.set_rts(true);
        let written = port.write(b"AT\r\n").expect("write");
        assert!(written > 0);
        let mut buf = [0u8; 64];
        let _ = port.read(&mut buf);
    }
    port.close();
}
