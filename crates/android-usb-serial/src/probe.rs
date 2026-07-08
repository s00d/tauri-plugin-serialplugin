//! Driver probing (ported from ProbeTable.java / UsbId.java).

use crate::transport::InterfaceInfo;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DriverType {
    CdcAcm,
    Cp21xx,
    Ftdi,
    Prolific,
    Ch34x,
    GsmModem,
    ChromeCcd,
}

#[derive(Debug, Clone)]
pub struct ProbeEntry {
    pub vendor_id: u16,
    pub product_id: u16,
    pub driver: DriverType,
}

#[derive(Debug, Clone, Default)]
pub struct ProbeTable {
    entries: Vec<ProbeEntry>,
}

impl ProbeTable {
    pub fn default_table() -> Self {
        let mut table = Self::default();
        // Order matches Java getDefaultProbeTable driver registration for probe-fn fallback.
        table.add_product(0x0403, 0x6001, DriverType::Ftdi);
        table.add_product(0x0403, 0x6010, DriverType::Ftdi);
        table.add_product(0x0403, 0x6011, DriverType::Ftdi);
        table.add_product(0x0403, 0x6014, DriverType::Ftdi);
        table.add_product(0x0403, 0x6015, DriverType::Ftdi);
        table.add_product(0x10C4, 0xEA60, DriverType::Cp21xx);
        table.add_product(0x10C4, 0xEA70, DriverType::Cp21xx);
        table.add_product(0x10C4, 0xEA71, DriverType::Cp21xx);
        table.add_product(0x067B, 0x2303, DriverType::Prolific);
        table.add_product(0x067B, 0x23A3, DriverType::Prolific);
        table.add_product(0x067B, 0x23B3, DriverType::Prolific);
        table.add_product(0x067B, 0x23C3, DriverType::Prolific);
        table.add_product(0x067B, 0x23D3, DriverType::Prolific);
        table.add_product(0x067B, 0x23E3, DriverType::Prolific);
        table.add_product(0x067B, 0x23F3, DriverType::Prolific);
        table.add_product(0x1A86, 0x7523, DriverType::Ch34x);
        table.add_product(0x1A86, 0x5523, DriverType::Ch34x);
        table.add_product(0x18D1, 0x5014, DriverType::ChromeCcd);
        table.add_product(0x1782, 0x4D10, DriverType::GsmModem);
        table.add_product(0x1782, 0x4D12, DriverType::GsmModem);
        table
    }

    pub fn add_product(&mut self, vendor_id: u16, product_id: u16, driver: DriverType) {
        self.entries.push(ProbeEntry {
            vendor_id,
            product_id,
            driver,
        });
    }

    pub fn find(
        &self,
        vendor_id: u16,
        product_id: u16,
        interfaces: &[InterfaceInfo],
    ) -> DriverType {
        if let Some(entry) = self
            .entries
            .iter()
            .find(|e| e.vendor_id == vendor_id && e.product_id == product_id)
        {
            return entry.driver;
        }
        if cdc_acm_port_count(interfaces) > 0 {
            return DriverType::CdcAcm;
        }
        DriverType::CdcAcm // unreachable for unknown; caller checks port_count
    }

    pub fn port_count(&self, driver: DriverType, interfaces: &[InterfaceInfo]) -> usize {
        match driver {
            DriverType::CdcAcm => cdc_acm_port_count(interfaces),
            DriverType::Ftdi | DriverType::Cp21xx => interfaces.len().max(1),
            DriverType::ChromeCcd => 3,
            DriverType::Ch34x | DriverType::Prolific | DriverType::GsmModem => 1,
        }
    }

    /// Port count when interface list is not yet known (enumeration / probe_table fixtures).
    pub fn port_count_product(
        &self,
        vendor_id: u16,
        product_id: u16,
        driver: DriverType,
        interfaces: &[InterfaceInfo],
    ) -> usize {
        if !interfaces.is_empty() {
            return self.port_count(driver, interfaces);
        }
        match (vendor_id, product_id) {
            (0x0403, 0x6010 | 0x6011 | 0x6014 | 0x6015) => 2,
            (0x10C4, 0xEA70 | 0xEA71) => 2,
            (0x18D1, 0x5014) => 3,
            _ => self.port_count(driver, interfaces),
        }
    }

    pub fn entries(&self) -> &[ProbeEntry] {
        &self.entries
    }
}

pub fn cdc_acm_port_count(interfaces: &[InterfaceInfo]) -> usize {
    let comm = interfaces
        .iter()
        .filter(|i| i.class == 2 && i.subclass == 2)
        .count();
    let data = interfaces.iter().filter(|i| i.class == 10).count();
    comm.min(data)
        .max(if comm == 0 && data == 0 { 0 } else { 1 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_table_finds_ftdi() {
        let table = ProbeTable::default_table();
        let driver = table.find(0x0403, 0x6001, &[]);
        assert_eq!(driver, DriverType::Ftdi);
    }

    #[test]
    fn cdc_probe_fn_fallback() {
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
        ];
        let driver = table.find(0x9999, 0x0001, &ifaces);
        assert_eq!(driver, DriverType::CdcAcm);
        assert_eq!(table.port_count(driver, &ifaces), 1);
    }
}
