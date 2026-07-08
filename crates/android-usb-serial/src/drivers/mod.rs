//! USB serial driver implementations.

mod cdc_acm;
mod ch34x;
mod chrome_ccd;
mod cp21xx;
mod ftdi;
mod gsm_modem;
mod prolific;

pub use cdc_acm::CdcAcmDriver;
pub use ch34x::Ch34xDriver;
pub use chrome_ccd::ChromeCcdDriver;
pub use cp21xx::Cp21xxDriver;
pub use ftdi::ftdi_baud_encoding;
pub use ftdi::FtdiDriver;
pub use gsm_modem::GsmModemDriver;
pub use prolific::ProlificDriver;

use crate::config::{FlowControl, LineConfig, PurgeKind};
use crate::error::Result;
use crate::probe::DriverType;
use crate::reader::SerialReader;
use crate::rx_filter::RxFilter;
use crate::transport::{BulkIn, BulkOut, SharedTransport};

pub const WRITE_TIMEOUT_MS: u32 = 5000;

#[derive(Debug, Clone, Copy, Default)]
pub struct ModemStatus {
    pub cts: bool,
    pub dsr: bool,
    pub ri: bool,
    pub cd: bool,
}

pub trait Driver: Send {
    fn open(&mut self, transport: &SharedTransport) -> Result<()>;
    fn close(&mut self) -> Result<()>;
    fn write(&mut self, data: &[u8]) -> Result<usize>;
    fn read(&mut self, buf: &mut [u8]) -> Result<usize>;
    fn set_line_config(&mut self, cfg: LineConfig) -> Result<()>;
    fn set_flow_control(&mut self, flow: FlowControl) -> Result<()>;
    fn set_dtr(&mut self, value: bool) -> Result<()>;
    fn set_rts(&mut self, value: bool) -> Result<()>;
    fn set_break(&mut self, enabled: bool) -> Result<()>;
    fn purge(&mut self, kind: PurgeKind) -> Result<()>;
    fn modem_status(&mut self) -> Result<ModemStatus>;
    fn bulk_in_mps(&self) -> u16;
    fn take_bulk_in(&mut self) -> Option<Box<dyn BulkIn>>;
    fn rx_filters(&self) -> Vec<Box<dyn RxFilter>> {
        Vec::new()
    }
    fn start_reader(&mut self) -> Result<SerialReader> {
        let bulk = self
            .take_bulk_in()
            .ok_or_else(|| crate::error::UsbSerialError::Io("no bulk in".into()))?;
        Ok(SerialReader::start(
            bulk,
            self.bulk_in_mps(),
            200,
            self.rx_filters(),
        ))
    }
}

pub fn create_driver(driver_type: DriverType, port_index: usize) -> Box<dyn Driver> {
    match driver_type {
        DriverType::CdcAcm => Box::new(CdcAcmDriver::new(port_index)),
        DriverType::Ftdi => Box::new(FtdiDriver::new(port_index)),
        DriverType::Cp21xx => Box::new(Cp21xxDriver::new(port_index)),
        DriverType::Ch34x => Box::new(Ch34xDriver::new(port_index)),
        DriverType::Prolific => Box::new(ProlificDriver::new(port_index)),
        DriverType::GsmModem => Box::new(GsmModemDriver::new(port_index)),
        DriverType::ChromeCcd => Box::new(ChromeCcdDriver::new(port_index)),
    }
}

pub fn line_coding_bytes(cfg: &LineConfig) -> [u8; 7] {
    let stop = match cfg.stop_bits {
        crate::config::StopBits::One => 0u8,
        crate::config::StopBits::OnePointFive => 1,
        crate::config::StopBits::Two => 2,
    };
    let parity = match cfg.parity {
        crate::config::Parity::None => 0,
        crate::config::Parity::Odd => 1,
        crate::config::Parity::Even => 2,
        crate::config::Parity::Mark => 3,
        crate::config::Parity::Space => 4,
    };
    let data = match cfg.data_bits {
        crate::config::DataBits::Five => 5,
        crate::config::DataBits::Six => 6,
        crate::config::DataBits::Seven => 7,
        crate::config::DataBits::Eight => 8,
    };
    [
        (cfg.baud_rate & 0xff) as u8,
        ((cfg.baud_rate >> 8) & 0xff) as u8,
        ((cfg.baud_rate >> 16) & 0xff) as u8,
        ((cfg.baud_rate >> 24) & 0xff) as u8,
        stop,
        parity,
        data,
    ]
}

struct EndpointPair {
    bulk_in: Option<Box<dyn BulkIn>>,
    bulk_out: Option<Box<dyn BulkOut>>,
    in_ep: u8,
    out_ep: u8,
    mps: u16,
}

impl EndpointPair {
    pub(crate) fn from_addresses(in_ep: u8, out_ep: u8, mps: u16) -> Self {
        Self {
            bulk_in: None,
            bulk_out: None,
            in_ep,
            out_ep,
            mps,
        }
    }

    fn open(transport: &SharedTransport, iface: u8) -> Result<Self> {
        let eps = transport.endpoints(iface);
        let in_ep = eps
            .iter()
            .find(|e| e.is_bulk_in())
            .ok_or_else(|| crate::error::UsbSerialError::ProbeFailed("no bulk in".into()))?;
        let out_ep = eps
            .iter()
            .find(|e| e.is_bulk_out())
            .ok_or_else(|| crate::error::UsbSerialError::ProbeFailed("no bulk out".into()))?;
        Ok(Self {
            bulk_in: None,
            bulk_out: None,
            in_ep: in_ep.address,
            out_ep: out_ep.address,
            mps: in_ep.max_packet_size,
        })
    }

    fn ensure_out(&mut self, transport: &SharedTransport) -> Result<()> {
        if self.bulk_out.is_none() {
            self.bulk_out = Some(transport.open_bulk_out(self.out_ep, self.mps)?);
        }
        Ok(())
    }

    fn ensure_in(&mut self, transport: &SharedTransport) -> Result<()> {
        if self.bulk_in.is_none() {
            self.bulk_in = Some(transport.open_bulk_in(self.in_ep, self.mps)?);
        }
        Ok(())
    }

    fn write(&mut self, transport: &SharedTransport, data: &[u8]) -> Result<usize> {
        // Only claim OUT — IN may already be owned by SerialReader after take_in().
        // Opening IN again hits nusb "endpoint already in use".
        self.ensure_out(transport)?;
        self.bulk_out
            .as_mut()
            .unwrap()
            .write(data, WRITE_TIMEOUT_MS)
    }

    fn take_in(&mut self, transport: &SharedTransport) -> Option<Box<dyn BulkIn>> {
        self.ensure_in(transport).ok()?;
        self.bulk_in.take()
    }
}
