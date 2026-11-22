//! 3GPP 27.010 basic-option frames, as a modem sends and reads them.

/// Set asynchronous balanced mode: opens a DLCI.
pub const SABM: u8 = 0x2F;
/// Unnumbered acknowledgement.
pub const UA: u8 = 0x63;
/// Disconnected mode: refuses a DLCI.
pub const DM: u8 = 0x0F;
/// Disconnect: closes a DLCI.
pub const DISC: u8 = 0x43;
/// Data.
pub const UIH: u8 = 0xEF;
/// Poll / final bit of the control octet.
pub const PF: u8 = 0x10;

/// Multiplexer close down: the control message on DLCI 0 that ends the multiplexer (type
/// octet, command form, as all of them here).
pub const CLD: u8 = 0xC3;
/// Flow control on: channel data may be sent again.
pub const FCON: u8 = 0xA3;
/// Flow control off: no channel data until FCON.
pub const FCOFF: u8 = 0x63;
/// Modem status command: V.24 signals and flow control of one DLCI.
pub const MSC: u8 = 0xE3;

const FLAG: u8 = 0xF9;

/// One frame.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Frame {
    /// Channel; 0 is control.
    pub dlci: u8,
    /// Command / response bit.
    pub cr: bool,
    /// Control octet without the P/F bit.
    pub kind: u8,
    /// Information field.
    pub payload: Vec<u8>,
}

impl Frame {
    /// A frame; `kind` is the control octet without the P/F bit.
    pub fn new(dlci: u8, cr: bool, kind: u8, payload: &[u8]) -> Self {
        Self {
            dlci,
            cr,
            kind,
            payload: payload.to_vec(),
        }
    }
}

/// 27.010 §5.2.1.6: reflected CRC-8, polynomial x^8 + x^2 + x + 1, ones complement.
fn fcs(header: &[u8]) -> u8 {
    let crc = header.iter().fold(0xFFu8, |crc, b| {
        (0..8).fold(
            crc ^ b,
            |c, _| if c & 1 == 1 { (c >> 1) ^ 0xE0 } else { c >> 1 },
        )
    });
    0xFF - crc
}

/// A frame as sent on the wire; `control` carries the P/F bit when it is set. The modem is
/// the responder (27.010 §5.2.1.2): `cr` is false in its commands (DISC) and data, true in
/// its responses (UA, DM).
pub fn frame(dlci: u8, cr: bool, control: u8, payload: &[u8]) -> Vec<u8> {
    let mut header = vec![dlci << 2 | u8::from(cr) << 1 | 1, control];
    match payload.len() {
        n @ 0..=127 => header.push((n as u8) << 1 | 1),
        n => header.extend([(n as u8 & 0x7F) << 1, (n >> 7) as u8]),
    }
    let check = fcs(&header);
    let mut out = vec![FLAG];
    out.extend(header);
    out.extend(payload);
    out.extend([check, FLAG]);
    out
}

/// Data from the modem on `dlci` (the modem is the responder: C/R 0 in its commands).
pub fn data(dlci: u8, payload: &[u8]) -> Vec<u8> {
    frame(dlci, false, UIH, payload)
}

/// A control message from the modem on DLCI 0.
pub fn control(kind: u8, value: &[u8]) -> Vec<u8> {
    let mut message = vec![kind, (value.len() as u8) << 1 | 1];
    message.extend(value);
    data(0, &message)
}

/// Splits a byte stream into frames; bytes that are no frame are dropped.
#[derive(Debug, Default)]
pub struct Reader {
    buf: Vec<u8>,
}

impl Reader {
    /// The frames completed by `bytes`.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<Frame> {
        self.push(bytes);
        std::iter::from_fn(|| self.next_frame()).collect()
    }

    pub(crate) fn push(&mut self, bytes: &[u8]) {
        self.buf.extend(bytes);
    }

    /// What is left behind the last frame.
    pub(crate) fn into_rest(self) -> Vec<u8> {
        let start = self.buf.iter().take_while(|b| **b == FLAG).count();
        self.buf[start..].to_vec()
    }

    /// The next complete frame.
    pub(crate) fn next_frame(&mut self) -> Option<Frame> {
        loop {
            let Some(start) = self.buf.iter().position(|b| *b == FLAG) else {
                self.buf.clear();
                return None;
            };
            self.buf.drain(..start);
            // Flags between frames may be shared or repeated.
            while self.buf.get(1) == Some(&FLAG) {
                self.buf.remove(0);
            }
            let head = self.buf.get(1..4)?;
            let (len, header_len) = if head[2] & 1 == 1 {
                (usize::from(head[2] >> 1), 3)
            } else {
                let high = self.buf.get(4)?;
                (usize::from(head[2] >> 1) | usize::from(*high) << 7, 4)
            };
            let end = 1 + header_len + len;
            let tail = self.buf.get(end..end + 2)?;
            let header = &self.buf[1..1 + header_len];
            if tail != [fcs(header), FLAG] {
                self.buf.remove(0);
                continue;
            }
            let frame = Frame {
                dlci: header[0] >> 2,
                cr: header[0] & 2 != 0,
                kind: header[1] & !PF,
                payload: self.buf[1 + header_len..end].to_vec(),
            };
            // The closing flag may open the next frame.
            self.buf.drain(..end + 1);
            return Some(frame);
        }
    }
}
