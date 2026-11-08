//! 3GPP TS 27.010 (GSM 07.10) basic-option frame encode/decode.
//!
//! Frame: `F9 | address | control | length (1–2 octets, EA) | info | FCS | F9`.
//! No byte stuffing: the length field delimits the info, so payload bytes may be 0xF9.
//! FCS is CRC-8 (reflected poly 0x07, i.e. 0xE0) over address+control+length; for UI
//! frames the info field is included too.

pub const FLAG: u8 = 0xF9;

/// Frame types (control field with the P/F bit cleared).
pub const SABM: u8 = 0x2F;
pub const UA: u8 = 0x63;
pub const DM: u8 = 0x0F;
pub const DISC: u8 = 0x43;
pub const UIH: u8 = 0xEF;
pub const UI: u8 = 0x03;
pub const PF: u8 = 0x10;

/// Largest info field the basic option can describe: the 2-octet length field holds
/// 15 bits (27.010 §5.2.1.5).
pub const MAX_INFO: usize = 32_767;
/// Default receive limit for the info field: room for a 1500-byte IP packet in PPP
/// framing. A corrupt length octet (noise, a lost flag) then costs one dropped frame,
/// not a stall until 32 KiB more arrive. Raise it with [`Deframer::with_max_info`] when
/// `AT+CMUX` sets a larger N1.
pub const DEFAULT_MAX_RX_INFO: usize = 1_509;
/// Info bytes per transmitted frame. Basic option defaults N1 to 31 unless `AT+CMUX`
/// sets it, so 31 is the only size every modem accepts.
// ponytail: fixed N1=31; parse N1 from the AT+CMUX command if throughput matters.
pub const TX_N1: usize = 31;

const fn crc_table() -> [u8; 256] {
    let mut table = [0u8; 256];
    let mut i = 0;
    while i < 256 {
        let mut crc = i as u8;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xE0
            } else {
                crc >> 1
            };
            bit += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
}

static CRC_TABLE: [u8; 256] = crc_table();

pub fn fcs(bytes: &[u8]) -> u8 {
    let crc = bytes
        .iter()
        .fold(0xFFu8, |crc, &b| CRC_TABLE[(crc ^ b) as usize]);
    0xFF - crc
}

pub fn encode_address(dlci: u8, cr: bool) -> u8 {
    ((dlci & 0x3F) << 2) | (u8::from(cr) << 1) | 0x01
}

pub fn decode_address(addr: u8) -> (u8, bool) {
    ((addr >> 2) & 0x3F, addr & 0x02 != 0)
}

/// One frame. `cr` is the address C/R bit; as initiator we set it on commands and UIH.
///
/// # Panics
/// If `info` is longer than [`MAX_INFO`] (use [`encode_uih`] to split data).
pub fn encode_frame(dlci: u8, cr: bool, control: u8, info: &[u8]) -> Vec<u8> {
    assert!(info.len() <= MAX_INFO, "cmux info field too long");
    let mut header = vec![encode_address(dlci, cr), control];
    let len = info.len();
    if len <= 127 {
        header.push(((len as u8) << 1) | 0x01);
    } else {
        header.push(((len & 0x7F) as u8) << 1);
        header.push((len >> 7) as u8);
    }
    let check = if control & !PF == UI {
        fcs(&[header.as_slice(), info].concat())
    } else {
        fcs(&header)
    };
    let mut frame = Vec::with_capacity(header.len() + len + 3);
    frame.push(FLAG);
    frame.extend_from_slice(&header);
    frame.extend_from_slice(info);
    frame.push(check);
    frame.push(FLAG);
    frame
}

/// UIH data frames for `payload`, split into [`TX_N1`]-sized info fields.
pub fn encode_uih(dlci: u8, payload: &[u8]) -> Vec<u8> {
    if payload.is_empty() {
        return encode_frame(dlci, true, UIH, &[]);
    }
    payload
        .chunks(TX_N1)
        .flat_map(|chunk| encode_frame(dlci, true, UIH, chunk))
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedFrame {
    pub dlci: u8,
    /// Address C/R bit.
    pub cr: bool,
    /// Control field with the P/F bit cleared (see [`SABM`], [`UA`], [`UIH`] …).
    pub kind: u8,
    pub payload: Vec<u8>,
}

/// Streaming deframer: keeps partial frames across reads and resyncs on garbage.
pub struct Deframer {
    buf: Vec<u8>,
    max_info: usize,
}

impl Default for Deframer {
    fn default() -> Self {
        Self::with_max_info(DEFAULT_MAX_RX_INFO)
    }
}

impl Deframer {
    /// Frames whose length field exceeds `max_info` (the N1 given to `AT+CMUX`, at most
    /// [`MAX_INFO`]) are dropped as corrupt.
    pub fn with_max_info(max_info: usize) -> Self {
        Self {
            buf: Vec::new(),
            max_info: max_info.clamp(1, MAX_INFO),
        }
    }

    pub fn feed(&mut self, chunk: &[u8]) -> Vec<DecodedFrame> {
        self.buf.extend_from_slice(chunk);
        let mut out = Vec::new();
        // Read offset into `buf`; consumed bytes are dropped once, at the end.
        let mut pos = 0;
        loop {
            // Skip everything before the next flag.
            match self.buf[pos..].iter().position(|&b| b == FLAG) {
                Some(i) => pos += i,
                None => {
                    pos = self.buf.len();
                    break;
                }
            }
            // Collapse runs of flags (closing flag + opening flag, or idle fill).
            let flags = self.buf[pos..].iter().take_while(|&&b| b == FLAG).count();
            pos += flags - 1;
            match self.parse_one(pos) {
                Parse::Need => break,
                // Not a frame start after all: skip this flag and rescan.
                Parse::Bad => pos += 1,
                Parse::Frame(frame, used) => {
                    // Keep the closing flag: it may also open the next frame.
                    pos += used - 1;
                    out.push(frame);
                }
            }
        }
        self.buf.drain(..pos);
        out
    }

    fn parse_one(&self, pos: usize) -> Parse {
        let b = &self.buf[pos..];
        if b.len() < 4 {
            return Parse::Need;
        }
        let (addr, control) = (b[1], b[2]);
        if addr & 0x01 == 0 {
            return Parse::Bad;
        }
        let (len, len_octets) = if b[3] & 0x01 != 0 {
            ((b[3] >> 1) as usize, 1)
        } else {
            if b.len() < 5 {
                return Parse::Need;
            }
            (((b[3] >> 1) as usize) | ((b[4] as usize) << 7), 2)
        };
        if len > self.max_info {
            return Parse::Bad;
        }
        let header_end = 3 + len_octets;
        let total = header_end + len + 2; // + FCS + closing flag
        if b.len() < total {
            return Parse::Need;
        }
        let info = &b[header_end..header_end + len];
        let kind = control & !PF;
        let expected = if kind == UI {
            fcs(&b[1..header_end + len])
        } else {
            fcs(&b[1..header_end])
        };
        if b[header_end + len] != expected || b[total - 1] != FLAG {
            return Parse::Bad;
        }
        let (dlci, cr) = decode_address(addr);
        Parse::Frame(
            DecodedFrame {
                dlci,
                cr,
                kind,
                payload: info.to_vec(),
            },
            total,
        )
    }
}

enum Parse {
    Need,
    Bad,
    Frame(DecodedFrame, usize),
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 27.010 reference: SABM with P on DLCI 0 is `F9 03 3F 01 1C F9`.
    #[test]
    fn sabm_dlci0_matches_spec_vector() {
        assert_eq!(
            encode_frame(0, true, SABM | PF, &[]),
            vec![0xF9, 0x03, 0x3F, 0x01, 0x1C, 0xF9]
        );
    }

    #[test]
    fn uih_roundtrip_with_flag_bytes_split_across_reads() {
        let payload = b"AT\r\xF9\xF9 data";
        let frame = encode_uih(1, payload);
        let mut def = Deframer::default();
        let (a, b) = frame.split_at(5);
        assert!(def.feed(a).is_empty());
        let frames = def.feed(b);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].dlci, 1);
        assert_eq!(frames[0].kind, UIH);
        assert_eq!(frames[0].payload, payload);
    }

    #[test]
    fn long_payload_is_split_into_n1_frames_and_back_to_back_frames_share_flags() {
        let payload: Vec<u8> = (0..100u8).collect();
        let mut stream = encode_uih(2, &payload);
        // A modem may send frames with a single shared flag between them.
        let second = encode_uih(3, b"OK");
        stream.extend_from_slice(&second[1..]);
        let frames = Deframer::default().feed(&stream);
        let data: Vec<u8> = frames
            .iter()
            .filter(|f| f.dlci == 2)
            .flat_map(|f| f.payload.clone())
            .collect();
        assert_eq!(data, payload);
        assert!(frames.iter().all(|f| f.payload.len() <= TX_N1));
        assert_eq!(frames.last().unwrap().payload, b"OK");
    }

    #[test]
    fn two_octet_length_roundtrip() {
        let info = vec![0x55u8; 200];
        let frame = encode_frame(5, true, UIH, &info);
        assert_eq!(frame[3] & 0x01, 0, "EA must be 0 for a 2-octet length");
        let frames = Deframer::default().feed(&frame);
        assert_eq!(frames[0].payload, info);
    }

    #[test]
    fn garbage_and_bad_fcs_are_skipped() {
        let mut stream = vec![0x00, 0xF9, 0x12, 0xF9];
        let mut bad = encode_uih(1, b"x");
        let fcs_at = bad.len() - 2;
        bad[fcs_at] ^= 0xFF;
        stream.extend_from_slice(&bad);
        stream.extend_from_slice(&encode_uih(1, b"good"));
        let frames = Deframer::default().feed(&stream);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].payload, b"good");
    }

    /// The old resync did `Vec::remove(0)` per bad flag: 1 MiB of `F9 00` took 17 s.
    #[test]
    fn resync_over_a_large_noise_read_is_linear() {
        let mut stream: Vec<u8> = [FLAG, 0x00].repeat(1 << 19);
        stream.extend_from_slice(&encode_uih(1, b"good"));
        let t = std::time::Instant::now();
        let frames = Deframer::default().feed(&stream);
        assert!(
            t.elapsed() < std::time::Duration::from_secs(5),
            "{:?}",
            t.elapsed()
        );
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].payload, b"good");
    }

    #[test]
    fn info_field_is_limited_to_15_bits() {
        let info = vec![0x55u8; MAX_INFO];
        let frames = Deframer::with_max_info(MAX_INFO).feed(&encode_frame(1, true, UIH, &info));
        assert_eq!(frames[0].payload, info);
        let too_long = std::panic::catch_unwind(|| encode_frame(1, true, UIH, &[0; MAX_INFO + 1]));
        assert!(too_long.is_err(), "32768 bytes would encode as length 0");
    }
}
