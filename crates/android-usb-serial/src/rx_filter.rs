//! RX filter chain (FTDI header strip, XON/XOFF inline).

use crate::xonxoff::XonXoffFilter;

const FTDI_READ_HEADER: usize = 2;

/// Strip FTDI 2-byte status headers from bulk IN data.
pub fn strip_ftdi_header(packet: &[u8], mps: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let mut pos = 0;
    while pos < packet.len() {
        let chunk = (pos + mps).min(packet.len());
        if chunk - pos <= FTDI_READ_HEADER {
            break;
        }
        out.extend_from_slice(&packet[pos + FTDI_READ_HEADER..chunk]);
        pos += mps;
    }
    out
}

pub trait RxFilter: Send {
    fn filter(&mut self, input: &[u8]) -> Vec<u8>;
}

pub struct FtdiHeaderFilter {
    mps: usize,
}

impl FtdiHeaderFilter {
    pub fn new(mps: u16) -> Self {
        Self {
            mps: mps.max(1) as usize,
        }
    }
}

impl RxFilter for FtdiHeaderFilter {
    fn filter(&mut self, input: &[u8]) -> Vec<u8> {
        strip_ftdi_header(input, self.mps)
    }
}

pub struct XonXoffRxFilter {
    inner: XonXoffFilter,
}

impl XonXoffRxFilter {
    pub fn new(enabled: bool) -> Self {
        Self {
            inner: XonXoffFilter::new(enabled),
        }
    }
}

impl RxFilter for XonXoffRxFilter {
    fn filter(&mut self, input: &[u8]) -> Vec<u8> {
        self.inner.filter(input)
    }
}

pub struct ChainedRxFilter {
    filters: Vec<Box<dyn RxFilter>>,
}

impl ChainedRxFilter {
    pub fn new(filters: Vec<Box<dyn RxFilter>>) -> Self {
        Self { filters }
    }
}

impl RxFilter for ChainedRxFilter {
    fn filter(&mut self, input: &[u8]) -> Vec<u8> {
        let mut data = input.to_vec();
        for f in &mut self.filters {
            data = f.filter(&data);
        }
        data
    }
}

pub fn apply_filters(filters: &mut [Box<dyn RxFilter>], input: &[u8]) -> Vec<u8> {
    let mut data = input.to_vec();
    for f in filters.iter_mut() {
        data = f.filter(&data);
    }
    data
}
