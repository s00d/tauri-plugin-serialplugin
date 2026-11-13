//! Shared fixtures for the integration tests: the proptest runner, the chip
//! catalogue over `FakeTransport`, and the per-chip reference model of control
//! transfers used by the driver and adapter models.
#![allow(dead_code, unused_imports)]

pub use android_usb_serial::config::{
    DataBits, FlowControl, LineConfig, Parity, PurgeKind, StopBits, CHAR_XOFF, CHAR_XON,
};
pub use android_usb_serial::drivers::{create_driver, ftdi_baud_encoding, Driver, ModemStatus};
pub use android_usb_serial::error::{ReadOutcome, UsbSerialError};
pub use android_usb_serial::fake::FakeTransport;
pub use android_usb_serial::probe::{DriverType, ProbeTable};
pub use android_usb_serial::rx_filter::apply_filters;
pub use android_usb_serial::transport::{
    BulkIn, EndpointInfo, InterfaceInfo, SharedTransport, Transport,
};
pub use android_usb_serial::{describe_device, open_port};
pub use proptest::prelude::*;
pub use proptest::test_runner::{Config, TestRunner};
pub use std::sync::Arc;
pub use std::time::{Duration, Instant};

/// Run `test` over `cases` inputs from `strategy`; on failure proptest shrinks
/// the input and the panic message prints the minimal failing case (Debug).
pub fn run_cases<S>(cases: u32, strategy: S, test: impl Fn(S::Value) -> Result<(), TestCaseError>)
where
    S: Strategy,
    S::Value: std::fmt::Debug,
{
    let mut runner = TestRunner::new(Config {
        cases,
        failure_persistence: None,
        max_shrink_iters: 4000,
        ..Config::default()
    });
    if let Err(e) = runner.run(&strategy, test) {
        panic!("{e}");
    }
}

/// Error variant only: tests compare kinds, never error text.
pub fn kind(e: &UsbSerialError) -> std::mem::Discriminant<UsbSerialError> {
    std::mem::discriminant(e)
}

pub fn shared(fake: &FakeTransport) -> SharedTransport {
    Arc::new(fake.clone())
}

pub fn iface(id: u8, class: u8, subclass: u8) -> InterfaceInfo {
    InterfaceInfo {
        id,
        class,
        subclass,
        protocol: 0,
    }
}

pub fn ep(address: u8, attributes: u8) -> EndpointInfo {
    EndpointInfo {
        address,
        attributes,
        max_packet_size: 64,
        interval: u8::from(attributes == 3),
    }
}

/// Poll `done` until it holds or the deadline passes (no sleeping).
pub fn eventually(mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if done() {
            return true;
        }
        std::thread::yield_now();
    }
    done()
}

/// One recorded control transfer: (bmRequestType, bRequest, wValue, wIndex, data).
pub type Ctl = (u8, u8, u16, u16, Vec<u8>);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlType {
    Hx,
    Hxn,
    T01,
    Ta,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chip {
    CdcSingle,
    CdcIad,
    CdcMulti(usize),
    Ft232r,
    /// FT232R layout with an H-series bcdDevice (7 = FT2232H, 8 = FT4232H, 9 = FT232H).
    Ft232h(u8),
    /// FT2232 (bcdDevice 7), port 0/1.
    Ft2232(usize),
    /// Two-interface FTDI that is not H-series (bcdDevice 5), port 0/1.
    Ft2232b(usize),
    Cp2102,
    Cp2105(usize),
    Ch340,
    Pl2303(PlType),
    Gsm,
    Ccd(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    Cdc,
    Ftdi,
    Cp,
    Ch,
    Pl(PlType),
    Gsm,
    Ccd,
}

impl Chip {
    pub fn family(self) -> Family {
        match self {
            Chip::CdcSingle | Chip::CdcIad | Chip::CdcMulti(_) => Family::Cdc,
            Chip::Ft232r | Chip::Ft232h(_) | Chip::Ft2232(_) | Chip::Ft2232b(_) => Family::Ftdi,
            Chip::Cp2102 | Chip::Cp2105(_) => Family::Cp,
            Chip::Ch340 => Family::Ch,
            Chip::Pl2303(t) => Family::Pl(t),
            Chip::Gsm => Family::Gsm,
            Chip::Ccd(_) => Family::Ccd,
        }
    }

    pub fn port(self) -> usize {
        match self {
            Chip::CdcMulti(p)
            | Chip::Ft2232(p)
            | Chip::Ft2232b(p)
            | Chip::Cp2105(p)
            | Chip::Ccd(p) => p,
            _ => 0,
        }
    }

    pub fn driver(self) -> DriverType {
        match self.family() {
            Family::Cdc => DriverType::CdcAcm,
            Family::Ftdi => DriverType::Ftdi,
            Family::Cp => DriverType::Cp21xx,
            Family::Ch => DriverType::Ch34x,
            Family::Pl(_) => DriverType::Prolific,
            Family::Gsm => DriverType::GsmModem,
            Family::Ccd => DriverType::ChromeCcd,
        }
    }

    pub fn port_count(self) -> usize {
        match self {
            Chip::CdcMulti(_) | Chip::Ft2232(_) | Chip::Ft2232b(_) | Chip::Cp2105(_) => 2,
            Chip::Ccd(_) => 3,
            _ => 1,
        }
    }

    pub fn fake(self) -> FakeTransport {
        let fake = self.bare_fake();
        if let Chip::Pl2303(_) = self {
            // A real device also reports configuration descriptors; detection must
            // read the device descriptor, not these.
            fake.set_raw_descriptors(vec![
                9, 2, 32, 0, 1, 1, 0, 0x80, 50, 9, 4, 0, 0, 3, 0xFF, 0, 0, 0,
            ]);
        }
        fake
    }

    fn bare_fake(self) -> FakeTransport {
        match self {
            Chip::CdcSingle => FakeTransport::cdc_single_iface(),
            Chip::CdcIad => FakeTransport::cdc_iad(),
            Chip::CdcMulti(_) => FakeTransport::cdc_multi(),
            Chip::Ft232r => FakeTransport::ftdi_ft232r(),
            Chip::Ft232h(bcd) => {
                let f = FakeTransport::ftdi_ft232r();
                f.patch_device_descriptor(|d| d[13] = bcd);
                f
            }
            Chip::Ft2232(_) => FakeTransport::ftdi_ft2232(),
            Chip::Ft2232b(_) => {
                let f = FakeTransport::ftdi_ft2232();
                f.patch_device_descriptor(|d| d[13] = 5);
                f
            }
            Chip::Cp2102 => FakeTransport::cp2102(),
            Chip::Cp2105(_) => FakeTransport::cp2105(),
            Chip::Ch340 => FakeTransport::ch340_dual_iface(),
            Chip::Pl2303(PlType::Hx) => {
                let f = FakeTransport::pl2303_hx();
                // USB 1.1 + 64-byte EP0: the classic HX.
                f.patch_device_descriptor(|d| {
                    d[2] = 0x10;
                    d[3] = 0x01;
                });
                f
            }
            Chip::Pl2303(PlType::Hxn) => FakeTransport::pl2303_hxn(),
            Chip::Pl2303(PlType::T01) => FakeTransport::pl2303_type01(),
            Chip::Pl2303(PlType::Ta) => FakeTransport::pl2303_ta(),
            Chip::Gsm => FakeTransport::gsm_modem(),
            Chip::Ccd(_) => FakeTransport::chrome_ccd_3port(),
        }
    }

    /// Interfaces the open must claim.
    pub fn claims(self) -> Vec<u8> {
        match self {
            Chip::CdcSingle => vec![0],
            Chip::CdcIad | Chip::CdcMulti(0) => vec![0, 1],
            Chip::CdcMulti(_) => vec![2, 3],
            Chip::Ch340 => vec![0, 1],
            Chip::Pl2303(_) | Chip::Gsm => vec![0],
            c => vec![c.port() as u8],
        }
    }

    pub fn out_ep(self) -> u8 {
        match self {
            Chip::CdcIad | Chip::CdcMulti(0) | Chip::Ch340 => 0x03,
            Chip::CdcMulti(_) => 0x05,
            Chip::Ft2232(p) | Chip::Ft2232b(p) | Chip::Cp2105(p) | Chip::Ccd(p) => {
                0x02 + 2 * p as u8
            }
            _ => 0x02,
        }
    }

    /// SERIAL_STATE / PL2303 status notifications arrive on an interrupt endpoint.
    pub fn notifies(self) -> bool {
        matches!(self, Chip::CdcIad | Chip::Pl2303(_))
    }

    pub fn cdc_index(self) -> u16 {
        if self == Chip::CdcMulti(1) {
            2
        } else {
            0
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exp {
    Ok,
    Unsupported,
    Io,
}

pub fn class<T>(r: &Result<T, UsbSerialError>) -> Exp {
    match r {
        Ok(_) => Exp::Ok,
        Err(UsbSerialError::Unsupported(_)) => Exp::Unsupported,
        Err(UsbSerialError::Io(_)) => Exp::Io,
        Err(e) => panic!("unexpected error class {e:?}"),
    }
}

pub type Lines = (bool, bool, bool, bool);

pub fn lines(m: ModemStatus) -> Lines {
    (m.cts, m.dsr, m.ri, m.cd)
}

pub const PL_STANDARD: &[u32] = &[
    75, 150, 300, 600, 1200, 1800, 2400, 3600, 4800, 7200, 9600, 14400, 19200, 28800, 38400, 57600,
    115200, 230400, 460800, 614400, 921600, 1228800, 2457600, 3000000, 6000000,
];

/// Reference PL2303 baud field (upstream ProlificSerialDriver.filterBaudRate).
pub fn pl_ref_baud(t: PlType, baud: u32) -> Option<u32> {
    if baud == 0 {
        return None;
    }
    if t == PlType::Hxn {
        // Bit 31 marks a divisor encoding, so a direct rate cannot use it.
        return (baud & (1 << 31) == 0).then_some(baud);
    }
    if PL_STANDARD.contains(&baud) {
        return Some(baud);
    }
    let base = 384_000_000u64;
    let mut m = (base / u64::from(baud)) as u32;
    if m == 0 {
        return None;
    }
    let mut exp = 0u32;
    let (buf, eff) = if t == PlType::Ta {
        while m >= 2048 {
            if exp >= 15 {
                return None;
            }
            m >>= 1;
            exp += 1;
        }
        let buf = m + ((exp & !1) << 12) + ((exp & 1) << 16) + (1 << 31);
        (buf, (base / u64::from(m)) >> exp)
    } else {
        while m >= 512 {
            if exp >= 7 {
                return None;
            }
            m >>= 2;
            exp += 1;
        }
        (
            m + (exp << 9) + (1 << 31),
            (base / u64::from(m)) >> (exp * 2),
        )
    };
    ((1.0 - eff as f64 / f64::from(baud)).abs() < 0.031).then_some(buf)
}

/// Reference CH34x baud registers (0x1312, 0x0F2C), upstream formula plus the
/// 3% deviation limit every other chip applies.
pub fn ch_ref_baud(baud: u32) -> Option<(u16, u16)> {
    match baud {
        0 => return None,
        921_600 => return Some((0xF387, 0)),
        307_200 => return Some((0xD987, 0)),
        _ => {}
    }
    const F: u64 = 1_532_620_800;
    let mut factor = F / u64::from(baud);
    let mut div = 3u32;
    while factor > 0xfff0 {
        if div == 0 {
            return None;
        }
        factor >>= 3;
        div -= 1;
    }
    if factor == 0 {
        return None;
    }
    let eff = F as f64 / (factor as f64 * f64::from(1u32 << (3 * (3 - div))));
    if (1.0 - eff / f64::from(baud)).abs() >= 0.031 {
        return None;
    }
    let reg = 0x10000 - factor;
    Some((
        ((reg & 0xff00) as u16) + 0x80 + div as u16,
        (reg & 0xff) as u16,
    ))
}

pub fn coding(cfg: &LineConfig, baud_field: u32) -> Vec<u8> {
    let mut v = baud_field.to_le_bytes().to_vec();
    v.push(match cfg.stop_bits {
        StopBits::One => 0,
        StopBits::OnePointFive => 1,
        StopBits::Two => 2,
    });
    v.push(parity_code(cfg.parity));
    v.push(data_bits(cfg.data_bits));
    v
}

pub fn parity_code(p: Parity) -> u8 {
    match p {
        Parity::None => 0,
        Parity::Odd => 1,
        Parity::Even => 2,
        Parity::Mark => 3,
        Parity::Space => 4,
    }
}

pub fn data_bits(d: DataBits) -> u8 {
    match d {
        DataBits::Five => 5,
        DataBits::Six => 6,
        DataBits::Seven => 7,
        DataBits::Eight => 8,
    }
}

pub fn strip_xonxoff(data: &[u8]) -> Vec<u8> {
    data.iter()
        .copied()
        .filter(|&b| b != 17 && b != 19)
        .collect()
}

/// Reference model of one open port.
pub struct Model {
    pub chip: Chip,
    pub mps: usize,
    pub opened_once: bool,
    pub open: bool,
    pub controls: Vec<Ctl>,
    pub bulk_out: Vec<(u8, Vec<u8>)>,
    pub dtr: bool,
    pub rts: bool,
    pub brk: bool,
    pub flow: FlowControl,
    pub ftdi_dc: u16,
    pub ch_rts_cts: bool,
    pub pl_cache: Option<(u32, u8, u8, u8)>,
    pub pl_status: Option<u8>,
    pub cdc_state: u16,
    pub pending_rx_error: Option<String>,
    /// The interrupt-endpoint thread (CDC notifications / PL2303 status) died on a read error.
    pub notify_dead: bool,
}

impl Model {
    pub fn new(chip: Chip, mps: usize) -> Self {
        Self {
            chip,
            mps,
            opened_once: false,
            open: false,
            controls: vec![],
            bulk_out: vec![],
            dtr: false,
            rts: false,
            brk: false,
            flow: FlowControl::None,
            ftdi_dc: 0,
            ch_rts_cts: false,
            pl_cache: None,
            pl_status: None,
            cdc_state: 0,
            pending_rx_error: None,
            notify_dead: false,
        }
    }

    pub fn reset_session(&mut self) {
        let (chip, mps, err) = (self.chip, self.mps, self.pending_rx_error.take());
        *self = Self::new(chip, mps);
        self.pending_rx_error = err;
        self.opened_once = true;
        self.open = true;
    }

    pub fn p(&self) -> u16 {
        self.chip.port() as u16
    }

    pub fn ctl(&mut self, rt: u8, req: u8, value: u16, index: u16, data: Vec<u8>) {
        self.controls.push((rt, req, value, index, data));
    }

    pub fn ftdi(&mut self, req: u8, value: u16, index: u16) {
        self.ctl(0x40, req, value, index, vec![]);
    }

    pub fn widx(&self) -> u16 {
        self.p() + 1
    }

    pub fn cp(&mut self, req: u8, value: u16, data: Vec<u8>) {
        let p = self.p();
        self.ctl(0x41, req, value, p, data);
    }

    pub fn ch(&mut self, req: u8, value: u16, index: u16) {
        self.ctl(0x40, req, value, index, vec![]);
    }

    pub fn pl_vendor_out(&mut self, t: PlType, value: u16, index: u16) {
        let req = if t == PlType::Hxn { 0x80 } else { 0x01 };
        self.ctl(0x40, req, value, index, vec![]);
    }

    pub fn pl_purge(&mut self, t: PlType, write: bool, read: bool) {
        if t == PlType::Hxn {
            let index = u16::from(write) | (u16::from(read) << 1);
            if index != 0 {
                self.pl_vendor_out(t, 0x07, index);
            }
        } else {
            if write {
                self.pl_vendor_out(t, 0x08, 0);
            }
            if read {
                self.pl_vendor_out(t, 0x09, 0);
            }
        }
    }

    pub fn cdc(&mut self, req: u8, value: u16, data: Vec<u8>) {
        let index = self.chip.cdc_index();
        self.ctl(0x21, req, value, index, data);
    }

    pub fn line(&mut self, cfg: &LineConfig) -> Exp {
        match self.chip.family() {
            Family::Cdc => {
                self.cdc(0x20, 0, coding(cfg, cfg.baud_rate));
                Exp::Ok
            }
            Family::Ftdi => {
                if cfg.baud_rate > 3_500_000 {
                    return Exp::Unsupported;
                }
                let with_port = matches!(
                    self.chip,
                    Chip::Ft2232(_) | Chip::Ft2232b(_) | Chip::Ft232h(_)
                );
                let Ok((v, i)) = ftdi_baud_encoding(cfg.baud_rate, with_port, self.chip.port())
                else {
                    return Exp::Unsupported;
                };
                self.ftdi(3, v, i);
                if matches!(cfg.data_bits, DataBits::Five | DataBits::Six)
                    || cfg.stop_bits == StopBits::OnePointFive
                {
                    return Exp::Unsupported;
                }
                let mut dc =
                    u16::from(data_bits(cfg.data_bits)) | (u16::from(parity_code(cfg.parity)) << 8);
                if cfg.stop_bits == StopBits::Two {
                    dc |= 0x1000;
                }
                if self.brk {
                    dc |= 0x4000;
                }
                self.ftdi_dc = dc;
                let w = self.widx();
                self.ftdi(4, dc, w);
                Exp::Ok
            }
            Family::Ch => {
                let Some((v1, v2)) = ch_ref_baud(cfg.baud_rate) else {
                    return Exp::Unsupported;
                };
                self.ch(0x9A, 0x1312, v1);
                self.ch(0x9A, 0x0F2C, v2);
                if cfg.stop_bits == StopBits::OnePointFive {
                    return Exp::Unsupported;
                }
                let mut lcr = 0xC0 | u16::from(data_bits(cfg.data_bits) - 5);
                if cfg.stop_bits == StopBits::Two {
                    lcr |= 0x04;
                }
                lcr |= match cfg.parity {
                    Parity::None => 0,
                    Parity::Odd => 0x08,
                    Parity::Even => 0x18,
                    Parity::Mark => 0x28,
                    Parity::Space => 0x38,
                };
                self.ch(0x9A, 0x2518, lcr);
                Exp::Ok
            }
            Family::Cp => {
                if cfg.baud_rate == 0 {
                    return Exp::Unsupported;
                }
                self.cp(0x1E, 0, cfg.baud_rate.to_le_bytes().to_vec());
                let restricted = self.chip == Chip::Cp2105(1);
                if restricted
                    && (cfg.data_bits != DataBits::Eight
                        || matches!(cfg.parity, Parity::Mark | Parity::Space)
                        || cfg.stop_bits == StopBits::Two)
                {
                    return Exp::Unsupported;
                }
                if cfg.stop_bits == StopBits::OnePointFive {
                    return Exp::Unsupported;
                }
                let mut bits = u16::from(data_bits(cfg.data_bits)) << 8;
                bits |= u16::from(parity_code(cfg.parity)) << 4;
                if cfg.stop_bits == StopBits::Two {
                    bits |= 2;
                }
                self.cp(0x03, bits, vec![]);
                Exp::Ok
            }
            Family::Pl(t) => {
                let Some(field) = pl_ref_baud(t, cfg.baud_rate) else {
                    return Exp::Unsupported;
                };
                let key = (
                    field,
                    data_bits(cfg.data_bits),
                    match cfg.stop_bits {
                        StopBits::One => 1,
                        StopBits::OnePointFive => 3,
                        StopBits::Two => 2,
                    },
                    parity_code(cfg.parity),
                );
                if self.pl_cache == Some(key) {
                    return Exp::Ok;
                }
                self.ctl(0x21, 0x20, 0, 0, coding(cfg, field));
                self.pl_purge(t, true, true);
                self.pl_cache = Some(key);
                Exp::Ok
            }
            Family::Gsm | Family::Ccd => Exp::Unsupported,
        }
    }

    pub fn flow(&mut self, f: FlowControl) -> Exp {
        match self.chip.family() {
            Family::Cdc | Family::Gsm | Family::Ccd => {
                if f == FlowControl::None {
                    Exp::Ok
                } else {
                    Exp::Unsupported
                }
            }
            Family::Ftdi => {
                let w = self.widx();
                let (value, index) = match f {
                    FlowControl::None => (0, w),
                    FlowControl::RtsCts => (0, w | 0x100),
                    FlowControl::DtrDsr => (0, w | 0x200),
                    FlowControl::XonXoffInline => (0x1311, w | 0x400),
                    FlowControl::XonXoff => return Exp::Unsupported,
                };
                self.ftdi(2, value, index);
                self.flow = f;
                Exp::Ok
            }
            Family::Ch => {
                let on = match f {
                    FlowControl::None => false,
                    FlowControl::RtsCts => true,
                    _ => return Exp::Unsupported,
                };
                if on != self.ch_rts_cts {
                    self.ch(0x9A, 0x2727, if on { 0x0101 } else { 0 });
                    self.ch_rts_cts = on;
                }
                Exp::Ok
            }
            Family::Cp => {
                if f == FlowControl::XonXoffInline {
                    return Exp::Unsupported;
                }
                let mut d = vec![0u8; 16];
                if f == FlowControl::RtsCts {
                    d[4] |= 0x80;
                    d[0] |= 0x08;
                } else if self.rts {
                    d[4] |= 0x40;
                }
                if f == FlowControl::DtrDsr {
                    d[0] |= 0x12;
                } else if self.dtr {
                    d[0] |= 0x01;
                }
                if f == FlowControl::XonXoff {
                    self.cp(0x19, 0, vec![0, 0, 0, 0, 17, 19]);
                    d[4] |= 0x03;
                    d[7] |= 0x80;
                    d[8] = 128;
                    d[12] = 128;
                }
                self.cp(0x13, 0, d);
                if f == FlowControl::XonXoff {
                    self.cp(0x09, 0, vec![]);
                }
                Exp::Ok
            }
            Family::Pl(t) => {
                let hxn = t == PlType::Hxn;
                let value = match f {
                    FlowControl::None => [0, 0xff],
                    FlowControl::RtsCts => [0x61, 0xfa],
                    FlowControl::XonXoffInline => [0xc1, 0xee],
                    _ => return Exp::Unsupported,
                };
                if hxn {
                    self.pl_vendor_out(t, 0x0a, value[1]);
                } else {
                    self.pl_vendor_out(t, 0, value[0]);
                }
                self.flow = f;
                Exp::Ok
            }
        }
    }

    pub fn line_bit(&mut self, dtr: bool, v: bool) -> Exp {
        if dtr {
            self.dtr = v;
        } else {
            self.rts = v;
        }
        match self.chip.family() {
            Family::Cdc => {
                let value = (u16::from(self.rts) << 1) | u16::from(self.dtr);
                self.cdc(0x22, value, vec![]);
            }
            Family::Ftdi => {
                let bit = if dtr { 0x01 } else { 0x02 };
                let w = self.widx();
                self.ftdi(1, (bit << 8) | if v { bit } else { 0 }, w);
            }
            Family::Ch => {
                let value = (u16::from(self.dtr) << 5) | (u16::from(self.rts) << 6);
                self.ch(0xA4, !value, 0);
            }
            Family::Cp => {
                let bit = if dtr { 0x01 } else { 0x02 };
                self.cp(0x07, (bit << 8) | if v { bit } else { 0 }, vec![]);
            }
            Family::Pl(_) => {
                let value = u16::from(self.dtr) | (u16::from(self.rts) << 1);
                self.ctl(0x21, 0x22, value, 0, vec![]);
            }
            Family::Gsm | Family::Ccd => return Exp::Unsupported,
        }
        Exp::Ok
    }

    pub fn brk(&mut self, on: bool, reply: &[u8]) -> Exp {
        match self.chip.family() {
            Family::Cdc => self.cdc(0x23, if on { 0xffff } else { 0 }, vec![]),
            Family::Ftdi => {
                self.brk = on;
                if on {
                    self.ftdi_dc |= 0x4000;
                } else {
                    self.ftdi_dc &= !0x4000;
                }
                let (dc, w) = (self.ftdi_dc, self.widx());
                self.ftdi(4, dc, w);
            }
            Family::Ch => {
                self.ctl(0xC0, 0x95, 0x1805, 0, vec![0, 0]);
                let [r0, r1, ..] = *reply else {
                    return Exp::Io;
                };
                let (r0, r1) = if on {
                    (r0 & !1, r1 & !0x40)
                } else {
                    (r0 | 1, r1 | 0x40)
                };
                self.ch(0x9A, 0x1805, (u16::from(r1) << 8) | u16::from(r0));
            }
            Family::Cp => self.cp(0x05, u16::from(on), vec![]),
            Family::Pl(_) => self.ctl(0x21, 0x23, if on { 0xffff } else { 0 }, 0, vec![]),
            Family::Gsm | Family::Ccd => return Exp::Unsupported,
        }
        Exp::Ok
    }

    pub fn purge(&mut self, k: PurgeKind) -> Exp {
        let (rx, tx) = match k {
            PurgeKind::Rx => (true, false),
            PurgeKind::Tx => (false, true),
            PurgeKind::Both => (true, true),
        };
        match self.chip.family() {
            Family::Cdc | Family::Ch => {}
            Family::Ftdi => {
                let w = self.widx();
                if rx {
                    self.ftdi(0, 1, w);
                }
                if tx {
                    self.ftdi(0, 2, w);
                }
            }
            Family::Cp => {
                let v = match k {
                    PurgeKind::Rx => 0x05,
                    PurgeKind::Tx => 0x0a,
                    PurgeKind::Both => 0x0f,
                };
                self.cp(0x12, v, vec![]);
            }
            Family::Pl(t) => self.pl_purge(t, rx, tx),
            Family::Gsm | Family::Ccd => return Exp::Unsupported,
        }
        Exp::Ok
    }

    /// Whether `modem_status` reads a control IN now (so a reply must be scripted).
    pub fn modem_reads_control_in(&self) -> bool {
        match self.chip.family() {
            Family::Ftdi | Family::Ch | Family::Cp => true,
            Family::Pl(_) => self.pl_status.is_none(),
            _ => false,
        }
    }

    pub fn modem(&mut self, reply: &[u8]) -> (Exp, Option<Lines>) {
        let first = reply.first().copied();
        let io = (Exp::Io, None);
        match self.chip.family() {
            // The notification reader keeps reporting its failure until close.
            Family::Cdc if self.notify_dead => io,
            Family::Cdc => {
                let s = self.cdc_state;
                (Exp::Ok, Some((false, s & 2 != 0, s & 8 != 0, s & 1 != 0)))
            }
            Family::Gsm | Family::Ccd => (Exp::Ok, Some((false, false, false, false))),
            Family::Ftdi => {
                let w = self.widx();
                self.ctl(0xC0, 5, 0, w, vec![0, 0]);
                first.map_or(io, |b| {
                    (
                        Exp::Ok,
                        Some((b & 0x10 != 0, b & 0x20 != 0, b & 0x40 != 0, b & 0x80 != 0)),
                    )
                })
            }
            Family::Cp => {
                let p = self.p();
                self.ctl(0xC1, 0x08, 0, p, vec![0]);
                first.map_or(io, |b| {
                    (
                        Exp::Ok,
                        Some((b & 0x10 != 0, b & 0x20 != 0, b & 0x40 != 0, b & 0x80 != 0)),
                    )
                })
            }
            Family::Ch => {
                self.ctl(0xC0, 0x95, 0x0706, 0, vec![0, 0]);
                first.map_or(io, |b| {
                    (
                        Exp::Ok,
                        Some((b & 1 == 0, b & 2 == 0, b & 4 == 0, b & 8 == 0)),
                    )
                })
            }
            Family::Pl(t) => {
                if self.pl_status.is_none() {
                    // Active-low vendor status bits → notification-format flags.
                    let (req, value, bits) = if t == PlType::Hxn {
                        (0x81, 0x80, [0x08, 0x20, 0x40, 0x80])
                    } else {
                        (0x01, 0x87, [0x08, 0x04, 0x02, 0x01])
                    };
                    self.ctl(0xC0, req, value, 0, vec![0]);
                    let Some(b) = first else { return io };
                    let flags = [0x80, 0x02, 0x01, 0x08]; // cts, dsr, cd, ri
                    let mut s = 0;
                    for (bit, flag) in bits.iter().zip(flags) {
                        if b & bit == 0 {
                            s |= flag;
                        }
                    }
                    self.pl_status = Some(s);
                }
                let s = self.pl_status.unwrap();
                (
                    Exp::Ok,
                    Some((s & 0x80 != 0, s & 0x02 != 0, s & 0x08 != 0, s & 0x01 != 0)),
                )
            }
        }
    }

    pub fn close(&mut self) {
        match self.chip.family() {
            Family::Cp => self.cp(0x00, 0, vec![]),
            Family::Pl(t) => self.pl_purge(t, true, true),
            _ => {}
        }
        self.open = false;
    }

    /// Bytes the chip puts on the wire for `payload`, and the bytes the host keeps.
    pub fn rx(&self, payload: &[u8], status: [u8; 2]) -> (Vec<u8>, Vec<u8>) {
        let inline = self.flow == FlowControl::XonXoffInline;
        match self.chip.family() {
            Family::Ftdi => {
                let mut wire = vec![];
                let mut pieces: Vec<&[u8]> = payload.chunks(self.mps - 2).collect();
                if pieces.is_empty() {
                    pieces.push(&[]);
                }
                for p in pieces {
                    wire.extend(status);
                    wire.extend(p);
                }
                let kept = if inline {
                    strip_xonxoff(payload)
                } else {
                    payload.to_vec()
                };
                (wire, kept)
            }
            Family::Pl(_) if inline => (payload.to_vec(), strip_xonxoff(payload)),
            _ => (payload.to_vec(), payload.to_vec()),
        }
    }

    /// An interrupt-endpoint thread shares the fake's injected read error.
    pub fn interrupt_thread_live(&self) -> bool {
        match self.chip {
            Chip::CdcIad => !self.notify_dead,
            Chip::Pl2303(_) => self.pl_status.is_some() && !self.notify_dead,
            _ => false,
        }
    }
}

pub fn set_bulk_mps(fake: &FakeTransport, mps: u16) {
    let layout: Vec<(u8, Vec<EndpointInfo>)> = fake
        .interfaces()
        .iter()
        .map(|i| {
            let eps = fake
                .endpoints(i.id)
                .into_iter()
                .map(|mut e| {
                    if e.attributes & 3 == 2 {
                        e.max_packet_size = mps;
                    }
                    e
                })
                .collect();
            (i.id, eps)
        })
        .collect();
    fake.configure_endpoints(&layout);
}

pub fn ctls(fake: &FakeTransport) -> Vec<Ctl> {
    fake.recorded_controls()
        .into_iter()
        .map(|c| (c.request_type, c.request, c.value, c.index, c.data))
        .collect()
}

/// Valid, invalid and per-chip edge baud rates.
pub fn baud() -> impl Strategy<Value = u32> {
    prop_oneof![
        prop::sample::select(vec![
            0u32,
            1,
            5,
            6,
            45,
            46,
            47,
            50,
            75,
            110,
            183,
            184,
            300,
            1200,
            9600,
            14400,
            23_391,
            23_392,
            57600,
            104_000,
            256_000,
            350_000,
            1_547_987,
            1_662_741,
            115_200,
            128_000,
            250_000,
            307_200,
            500_000,
            614_400,
            921_600,
            1_000_000,
            1_228_800,
            1_500_000,
            1_749_999,
            1_750_000,
            2_000_000,
            2_457_600,
            2_499_999,
            2_500_000,
            3_000_000,
            3_500_000,
            3_500_001,
            6_000_000,
            12_000_000,
            46_000_000,
            50_000_000,
            384_000_000,
            1_532_620_800,
            2_147_483_647,
            2_147_483_648,
            u32::MAX,
        ]),
        1u32..4_000_000,
        any::<u32>(),
    ]
}

pub fn line_config() -> impl Strategy<Value = LineConfig> {
    (
        baud(),
        prop::sample::select(vec![
            DataBits::Five,
            DataBits::Six,
            DataBits::Seven,
            DataBits::Eight,
        ]),
        prop::sample::select(vec![
            Parity::None,
            Parity::Odd,
            Parity::Even,
            Parity::Mark,
            Parity::Space,
        ]),
        prop::sample::select(vec![StopBits::One, StopBits::OnePointFive, StopBits::Two]),
    )
        .prop_map(|(baud_rate, data_bits, parity, stop_bits)| LineConfig {
            baud_rate,
            data_bits,
            parity,
            stop_bits,
        })
}
