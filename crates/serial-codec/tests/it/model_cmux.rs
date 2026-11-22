//! Model: 27.010 basic-option frames.
//!
//! proptest draws what a link carries: frames of every type (the closing flag may open the next
//! one), split data, idle flags, noise that starts no frame, frames over the receive limit.
//! The model writes each frame from the specification (with a bit-by-bit FCS); the encoder must
//! give the same bytes and the deframer exactly the deliverable frames, in any reads.

use crate::common::*;
use serial_codec::cmux::{
    decode_address, encode_address, encode_frame, encode_uih, fcs, DecodedFrame, Deframer,
    DEFAULT_MAX_RX_INFO, DISC, DM, FLAG, MAX_INFO, PF, SABM, TX_N1, UA, UI, UIH,
};

/// 27.010 §5.2.1.6: reflected CRC-8 (x^8+x^2+x+1), initial 0xFF, ones' complement.
fn reference_fcs(bytes: &[u8]) -> u8 {
    !bytes.iter().fold(0xFF, |crc: u8, b| {
        (0..8).fold(crc ^ b, |c, _| (c >> 1) ^ ((c & 1) * 0xE0))
    })
}

/// A frame as the host asks for it: any DLCI octet (the address holds six bits), C/R, control.
#[derive(Debug, Clone)]
struct Frame {
    dlci: u8,
    cr: bool,
    control: u8,
    info: Vec<u8>,
}

impl Frame {
    fn address(&self) -> u8 {
        (self.dlci & 0x3F) << 2 | u8::from(self.cr) << 1 | 1
    }

    /// Flag, address, control, length of 1 or 2 octets (EA bit), info, FCS, flag.
    fn wire(&self) -> Vec<u8> {
        let n = self.info.len();
        let mut b = vec![FLAG, self.address(), self.control];
        if n < 128 {
            b.push((n as u8) << 1 | 1);
        } else {
            b.extend([(n as u8) << 1, (n >> 7) as u8]);
        }
        let header = b.len();
        b.extend(&self.info);
        // The info field is covered in UI frames only.
        let covered = if self.control & !PF == UI {
            b.len()
        } else {
            header
        };
        b.push(reference_fcs(&b[1..covered]));
        b.push(FLAG);
        b
    }

    fn decoded(&self) -> DecodedFrame {
        DecodedFrame {
            dlci: self.dlci & 0x3F,
            cr: self.cr,
            kind: self.control & !PF,
            payload: self.info.clone(),
        }
    }
}

fn frame() -> impl Strategy<Value = Frame> {
    (
        any::<u8>(),
        any::<bool>(),
        select(vec![SABM, UA, DM, DISC, UIH, UI]),
        any::<bool>(),
        prop_oneof![
            3 => vec(any::<u8>(), 0..=127),
            1 => vec(any::<u8>(), 128..=DEFAULT_MAX_RX_INFO),
            // Flags in the info field: the length delimits it.
            1 => vec(Just(FLAG), 0..40),
        ],
    )
        .prop_map(|(dlci, cr, kind, pf, info)| Frame {
            dlci,
            cr,
            control: if pf { kind | PF } else { kind },
            info,
        })
        // The address octet 0xF9 (DLCI 62, C/R 0) reads as a flag: story_cmux pins it.
        .prop_filter("address is the flag", |f| f.address() != FLAG)
}

#[derive(Debug, Clone)]
enum Item {
    /// A frame; `true`: the previous closing flag opens it.
    Frame(Frame, bool),
    /// Data for a DLCI, split by the encoder.
    Data(u8, Vec<u8>),
    /// An info field over the receive limit, by this much.
    Oversized(usize),
    Idle(usize),
    /// No frame starts here: no flag in it, and its first octet is no address (EA clear).
    Noise(Vec<u8>),
}

fn item() -> impl Strategy<Value = Item> {
    let noise = vec(any::<u8>(), 0..8).prop_map(|mut v| {
        v.retain(|&b| b != FLAG);
        if let Some(b) = v.first_mut() {
            *b &= !1;
        }
        Item::Noise(v)
    });
    prop_oneof![
        5 => (frame(), any::<bool>()).prop_map(|(f, share)| Item::Frame(f, share)),
        2 => (1u8..62, vec(any::<u8>(), 0..300)).prop_map(|(d, p)| Item::Data(d, p)),
        1 => (1usize..500).prop_map(Item::Oversized),
        1 => (1usize..5).prop_map(Item::Idle),
        1 => noise,
    ]
}

fn feed(d: &mut Deframer, data: &[u8], sizes: &[usize]) -> Vec<DecodedFrame> {
    chunks(data, sizes)
        .into_iter()
        .flat_map(|c| d.feed(c))
        .collect()
}

#[test]
fn frames_survive_the_link() {
    let limit = prop_oneof![Just(DEFAULT_MAX_RX_INFO), 0usize..400];
    let strategy = (vec(item(), 0..8), vec(1usize..64, 1..6), limit);
    run_model(256, strategy, |(items, sizes, limit)| {
        let (mut stream, mut want) = (Vec::new(), Vec::new());
        let fits = |n: usize| n <= limit.max(1);
        for item in &items {
            match item {
                Item::Frame(f, share) => {
                    let wire = f.wire();
                    prop_assert_eq!(&encode_frame(f.dlci, f.cr, f.control, &f.info), &wire);
                    prop_assert_eq!(encode_address(f.dlci, f.cr), f.address());
                    prop_assert_eq!(decode_address(f.address()), (f.dlci & 0x3F, f.cr));
                    prop_assert_eq!(fcs(&wire[1..4]), reference_fcs(&wire[1..4]));
                    // A flag inside a dropped frame is a false start the model does not follow.
                    if !fits(f.info.len()) && wire[2..wire.len() - 2].contains(&FLAG) {
                        continue;
                    }
                    let skip = usize::from(*share && stream.last() == Some(&FLAG));
                    stream.extend_from_slice(&wire[skip..]);
                    if fits(f.info.len()) {
                        want.push(f.decoded());
                    }
                }
                Item::Data(dlci, payload) => {
                    let wire = encode_uih(*dlci, payload);
                    let mut parts: Vec<&[u8]> = payload.chunks(TX_N1).collect();
                    if parts.is_empty() {
                        parts.push(&[]);
                    }
                    let frames: Vec<Frame> = parts
                        .iter()
                        .map(|p| Frame {
                            dlci: *dlci,
                            cr: true,
                            control: UIH,
                            info: p.to_vec(),
                        })
                        .collect();
                    let bytes: Vec<u8> = frames.iter().flat_map(Frame::wire).collect();
                    prop_assert_eq!(&wire, &bytes);
                    if frames.iter().any(|f| !fits(f.info.len())) {
                        continue;
                    }
                    stream.extend(wire);
                    want.extend(frames.iter().map(Frame::decoded));
                }
                Item::Oversized(by) => {
                    let info = vec![0x55; limit.max(1) + by];
                    stream.extend(encode_frame(1, true, UIH, &info));
                }
                Item::Idle(n) => stream.extend(vec![FLAG; *n]),
                Item::Noise(n) => stream.extend(n),
            }
        }
        let mut d = Deframer::with_max_info(limit);
        prop_assert_eq!(feed(&mut d, &stream, &sizes), want.clone());
        if limit == DEFAULT_MAX_RX_INFO {
            prop_assert_eq!(Deframer::default().feed(&stream), want);
        }
        Ok(())
    });
}

/// Any bytes: every chunking gives the same frames, never more than the bytes can hold, none
/// over the limit; a frame with a changed octet (one the FCS covers, or a flag) is not delivered.
#[test]
fn any_bytes_in_any_reads() {
    let part = prop_oneof![
        frame().prop_map(|f| f.wire()),
        vec(any::<u8>(), 0..12),
        vec(select(vec![FLAG, 0x01, 0x03, 0xEF, 0x3F]), 0..12),
    ];
    let strategy = (
        vec(part, 0..8),
        vec(1usize..64, 1..6),
        prop_oneof![Just(DEFAULT_MAX_RX_INFO), 0usize..300, Just(usize::MAX)],
        (frame(), any::<prop::sample::Index>(), 1u8..=255),
    );
    run_model(256, strategy, |(parts, sizes, max, (f, at, flip))| {
        let stream = parts.concat();
        let whole = Deframer::with_max_info(max).feed(&stream);
        for sizes in [&sizes[..], &[1]] {
            let got = feed(&mut Deframer::with_max_info(max), &stream, sizes);
            prop_assert_eq!(got, whole.clone());
        }
        let limit = max.clamp(1, MAX_INFO);
        prop_assert!(whole
            .iter()
            .all(|f| f.payload.len() <= limit && f.dlci < 64));
        prop_assert!(whole.len() * 6 <= stream.len() + whole.len());

        let mut bad = f.wire();
        let info = 4 + usize::from(f.info.len() > 127)..bad.len() - 2;
        let i = at.index(bad.len());
        if f.control & !PF == UI || !info.contains(&i) {
            bad[i] ^= flip;
            let got = Deframer::default().feed(&bad);
            prop_assert!(!got.contains(&f.decoded()), "octet {} of {:02X?}", i, bad);
        }
        Ok(())
    });
}
