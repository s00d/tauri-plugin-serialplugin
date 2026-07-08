//! fd → dup → open path (FakeTransport stand-in until hardware spike).

#[cfg(feature = "fake-transport")]
#[test]
fn fake_transport_open_write() {
    use android_usb_serial::config::{DataBits, LineConfig, Parity, StopBits};
    use android_usb_serial::device::open_port;
    use android_usb_serial::fake::FakeTransport;
    use android_usb_serial::transport::{EndpointInfo, InterfaceInfo, Transport};
    use std::sync::Arc;

    let fake = FakeTransport::cdc_single_iface();
    fake.set_interfaces(vec![
        InterfaceInfo {
            id: 0,
            class: 2,
            subclass: 2,
            protocol: 0,
        },
        InterfaceInfo {
            id: 1,
            class: 10,
            subclass: 0,
            protocol: 0,
        },
    ]);
    fake.configure_endpoints(&[(
        1,
        vec![
            EndpointInfo {
                address: 0x81,
                attributes: 2,
                max_packet_size: 64,
                interval: 0,
            },
            EndpointInfo {
                address: 0x02,
                attributes: 2,
                max_packet_size: 64,
                interval: 0,
            },
        ],
    )]);
    let transport: Arc<dyn Transport> = Arc::new(fake.clone());
    let mut port = open_port(transport, 0).unwrap();
    port.set_line_config(LineConfig {
        baud_rate: 115_200,
        data_bits: DataBits::Eight,
        parity: Parity::None,
        stop_bits: StopBits::One,
    })
    .unwrap();
    assert_eq!(port.write(b"PING").unwrap(), 4);
    assert_eq!(fake.take_tx(), b"PING");
}

/// Real-device spike: `dup → from_fd → descriptor → claim → echo`.
/// Run on Android hardware: `cargo test -p android-usb-serial spike_fd_hardware -- --ignored`
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
