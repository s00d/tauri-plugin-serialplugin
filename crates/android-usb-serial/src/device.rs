//! Device discovery and port enumeration.

use crate::drivers::create_driver;
use crate::error::{Result, UsbSerialError};
use crate::probe::{DriverType, ProbeTable};
use crate::transport::SharedTransport;

#[derive(Debug, Clone)]
pub struct PortDescriptor {
    pub port_index: usize,
    pub driver: DriverType,
    pub vendor_id: u16,
    pub product_id: u16,
}

#[derive(Debug, Clone)]
pub struct DeviceDescriptor {
    pub vendor_id: u16,
    pub product_id: u16,
    pub driver: DriverType,
    pub ports: Vec<PortDescriptor>,
}

pub fn describe_device(transport: &SharedTransport) -> Result<DeviceDescriptor> {
    let desc = transport.raw_device_descriptor();
    let vendor_id = u16::from_le_bytes([desc[8], desc[9]]);
    let product_id = u16::from_le_bytes([desc[10], desc[11]]);
    let table = ProbeTable::default_table();
    let ifaces = transport.interfaces();
    let driver = table.find(vendor_id, product_id, &ifaces);
    let count = table.port_count(driver, &ifaces).max(1);
    let ports = (0..count)
        .map(|i| PortDescriptor {
            port_index: i,
            driver,
            vendor_id,
            product_id,
        })
        .collect();
    Ok(DeviceDescriptor {
        vendor_id,
        product_id,
        driver,
        ports,
    })
}

pub fn open_port(
    transport: SharedTransport,
    port_index: usize,
) -> Result<crate::port::SerialPortHandle> {
    let device = describe_device(&transport)?;
    let port = device
        .ports
        .get(port_index)
        .ok_or_else(|| UsbSerialError::Unsupported(format!("port {port_index}")))?;
    let mut driver = create_driver(port.driver, port_index);
    driver.open(&transport)?;
    Ok(crate::port::SerialPortHandle::new(transport, driver))
}
