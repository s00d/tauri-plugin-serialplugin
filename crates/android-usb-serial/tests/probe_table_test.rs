//! Probe table port-count rules (device_filter + product defaults).

use android_usb_serial::probe::{DriverType, ProbeTable};
use android_usb_serial::transport::InterfaceInfo;

#[test]
fn ftdi_ft2232_port_count_two_without_interfaces() {
    let table = ProbeTable::default_table();
    assert_eq!(
        table.port_count_product(0x0403, 0x6010, DriverType::Ftdi, &[]),
        2
    );
}

#[test]
fn cp2105_port_count_two_without_interfaces() {
    let table = ProbeTable::default_table();
    assert_eq!(
        table.port_count_product(0x10C4, 0xEA70, DriverType::Cp21xx, &[]),
        2
    );
}

#[test]
fn chrome_ccd_port_count_three() {
    let table = ProbeTable::default_table();
    assert_eq!(
        table.port_count_product(0x18D1, 0x5014, DriverType::ChromeCcd, &[]),
        3
    );
}

#[test]
fn cdc_acm_port_count_from_interfaces() {
    let table = ProbeTable::default_table();
    let ifaces = vec![
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
        InterfaceInfo {
            id: 2,
            class: 2,
            subclass: 2,
            protocol: 0,
        },
        InterfaceInfo {
            id: 3,
            class: 10,
            subclass: 0,
            protocol: 0,
        },
    ];
    assert_eq!(table.port_count(DriverType::CdcAcm, &ifaces), 2);
}

#[test]
fn ftdi_port_count_follows_interface_count() {
    let table = ProbeTable::default_table();
    let ifaces = vec![
        InterfaceInfo {
            id: 0,
            class: 255,
            subclass: 0,
            protocol: 0,
        },
        InterfaceInfo {
            id: 1,
            class: 255,
            subclass: 0,
            protocol: 0,
        },
    ];
    assert_eq!(table.port_count(DriverType::Ftdi, &ifaces), 2);
}
