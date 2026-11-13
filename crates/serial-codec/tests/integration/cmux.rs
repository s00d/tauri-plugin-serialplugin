//! 27.010 CMUX frames as properties: encode → [`Deframer`] round trips for every frame
//! type and both length forms, chunking independence, resync over noise, and the
//! receive limit.

use crate::common::*;
use serial_codec::cmux::{
    decode_address, encode_address, encode_frame, encode_uih, DecodedFrame, Deframer,
    DEFAULT_MAX_RX_INFO, DISC, DM, FLAG, MAX_INFO, PF, SABM, TX_N1, UA, UI, UIH,
};

/// (dlci, cr, control, info): every frame type, P/F on or off, info of a 1-octet
/// (0–127) or a 2-octet length (128+).
fn frame() -> impl Strategy<Value = (u8, bool, u8, Vec<u8>)> {
    (
        0u8..64,
        any::<bool>(),
        select(vec![SABM, UA, DM, DISC, UIH, UI]),
        any::<bool>(),
        prop_oneof![
            3 => vec(any::<u8>(), 0..=127),
            1 => vec(any::<u8>(), 128..=DEFAULT_MAX_RX_INFO),
            // Payloads full of flag bytes: the length field delimits, not the flags.
            1 => vec(Just(FLAG), 0..40),
        ],
    )
        // DLCI 62 with C/R 0 has address octet 0xF9, the flag: basic-option framing
        // cannot tell it from a flag run (see `address_equal_to_the_flag`).
        .prop_filter("address is not the flag", |f| {
            encode_address(f.0, f.1) != FLAG
        })
        .prop_map(|(dlci, cr, kind, pf, info)| (dlci, cr, kind | if pf { PF } else { 0 }, info))
}

fn decoded(f: &(u8, bool, u8, Vec<u8>)) -> DecodedFrame {
    DecodedFrame {
        dlci: f.0,
        cr: f.1,
        kind: f.2 & !PF,
        payload: f.3.clone(),
    }
}

/// Noise before the first frame. Noise between frames is left out: a false frame
/// start in it may wait for a length's worth of bytes (see `resyncs_after_noise`).
fn noise() -> impl Strategy<Value = Vec<u8>> {
    vec(any::<u8>().prop_filter("no flag", |&b| b != FLAG), 0..8)
}

fn feed(d: &mut Deframer, data: &[u8], sizes: &[usize]) -> Vec<DecodedFrame> {
    chunks(data, sizes)
        .into_iter()
        .flat_map(|c| d.feed(c))
        .collect()
}

proptest! {
    #![proptest_config(cases(256))]

    /// Frames, back to back (sharing a flag or not) after flag-free noise, come out of
    /// the deframer exactly as encoded, whatever the read chunking.
    #[test]
    fn frames_round_trip(
        lead in noise(),
        frames in vec((frame(), any::<bool>()), 0..8),
        sizes in vec(1usize..64, 1..6),
    ) {
        let mut stream = lead;
        for (f, share) in &frames {
            let bytes = encode_frame(f.0, f.1, f.2, &f.3);
            let len = f.3.len();
            prop_assert_eq!(bytes.len(), len + if len <= 127 { 6 } else { 7 });
            prop_assert_eq!(bytes[3] & 1, u8::from(len <= 127), "EA bit of the length");
            prop_assert_eq!((bytes[0], *bytes.last().unwrap()), (FLAG, FLAG));
            // A shared flag: the previous closing flag also opens this frame.
            let skip = usize::from(*share && stream.last() == Some(&FLAG));
            stream.extend_from_slice(&bytes[skip..]);
        }
        let want: Vec<DecodedFrame> = frames.iter().map(|(f, ..)| decoded(f)).collect();
        prop_assert_eq!(feed(&mut Deframer::default(), &stream, &sizes), want.clone());
        prop_assert_eq!(Deframer::default().feed(&stream), want);
    }

    /// Any byte stream: one read, arbitrary reads and byte-by-byte reads give the same
    /// frames, never more than the stream can hold, none over the receive limit.
    #[test]
    fn chunking_never_changes_frames(
        parts in vec(prop_oneof![
            frame().prop_map(|f| encode_frame(f.0, f.1, f.2, &f.3)),
            vec(any::<u8>(), 0..12),
            vec(select(vec![FLAG, 0x01, 0x03, 0xEF, 0x3F]), 0..12),
        ], 0..8),
        sizes in vec(1usize..64, 1..6),
        max in prop_oneof![Just(DEFAULT_MAX_RX_INFO), 0usize..300, Just(usize::MAX)],
    ) {
        let stream = parts.concat();
        let whole = Deframer::with_max_info(max).feed(&stream);
        prop_assert_eq!(feed(&mut Deframer::with_max_info(max), &stream, &sizes), whole.clone());
        prop_assert_eq!(feed(&mut Deframer::with_max_info(max), &stream, &[1]), whole.clone());
        let limit = max.clamp(1, MAX_INFO);
        prop_assert!(whole.iter().all(|f| f.payload.len() <= limit && f.dlci < 64));
        prop_assert!(whole.len() * 6 <= stream.len() + whole.len());
    }

    /// A frame is accepted exactly when its info fits the receive limit.
    #[test]
    fn receive_limit(len in 0usize..400, max in 0usize..400, f in frame()) {
        let info = vec![0x55; len];
        let frames = Deframer::with_max_info(max).feed(&encode_frame(f.0, f.1, f.2, &info));
        prop_assert_eq!(frames.len(), usize::from(len <= max.max(1)));
    }

    /// A corrupted octet anywhere the FCS covers (info only in UI frames) or in a flag
    /// means the frame is not delivered.
    #[test]
    fn corruption_drops_the_frame(f in frame(), at in any::<prop::sample::Index>(), flip in 1u8..=255) {
        let mut bad = encode_frame(f.0, f.1, f.2, &f.3);
        let info = 4 + usize::from(f.3.len() > 127)..bad.len() - 2;
        let i = at.index(bad.len());
        prop_assume!(f.2 & !PF == UI || !info.contains(&i));
        bad[i] ^= flip;
        prop_assert!(!Deframer::default().feed(&bad).contains(&decoded(&f)));
    }

    /// Long data is split into N1-sized UIH frames and joins back.
    #[test]
    fn uih_splits_into_n1_frames(dlci in 0u8..64, payload in vec(any::<u8>(), 0..300)) {
        let frames = Deframer::default().feed(&encode_uih(dlci, &payload));
        prop_assert_eq!(frames.len(), payload.len().div_ceil(TX_N1).max(1));
        prop_assert!(frames.iter().all(|f| f.dlci == dlci && f.cr && f.kind == UIH && f.payload.len() <= TX_N1));
        prop_assert_eq!(frames.iter().flat_map(|f| f.payload.clone()).collect::<Vec<u8>>(), payload);
    }

    #[test]
    fn address_round_trip(dlci in any::<u8>(), cr in any::<bool>()) {
        let a = encode_address(dlci, cr);
        prop_assert_eq!(a & 1, 1, "EA bit");
        prop_assert_eq!(decode_address(a), (dlci & 0x3F, cr));
    }
}

/// Given 1 MiB of `F9 00` noise (every flag a false frame start) in one read
/// When a valid frame follows it
/// Then the deframer resyncs over the noise and delivers that frame
#[test]
fn resyncs_after_noise() {
    let mut stream: Vec<u8> = [FLAG, 0x00].repeat(1 << 19);
    stream.extend_from_slice(&encode_uih(1, b"good"));
    let frames = Deframer::default().feed(&stream);
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].payload, b"good");
}

/// Given an info field of exactly 32 767 octets (the 15-bit length maximum)
/// When it is encoded and deframed, and one octet more is encoded
/// Then the first round-trips and the second panics instead of encoding length 0
#[test]
fn info_field_is_limited_to_15_bits() {
    let info = vec![0x55u8; MAX_INFO];
    let frames = Deframer::with_max_info(MAX_INFO).feed(&encode_frame(1, true, UIH, &info));
    assert_eq!(frames[0].payload, info);
    let too_long = std::panic::catch_unwind(|| encode_frame(1, true, UIH, &[0; MAX_INFO + 1]));
    assert!(too_long.is_err(), "32768 bytes would encode as length 0");
}

/// Given a frame for DLCI 62 with C/R 0, whose address octet is 0xF9 (the flag)
/// When it is deframed
/// Then it is not delivered: the deframer reads the address as a repeated flag. A
/// known limit of the basic option without transparency, pinned so a change shows.
#[test]
fn address_equal_to_the_flag() {
    assert_eq!(encode_address(62, false), FLAG);
    assert!(Deframer::default()
        .feed(&encode_frame(62, false, UIH, b"x"))
        .is_empty());
    assert_eq!(
        Deframer::default()
            .feed(&encode_frame(62, true, UIH, b"x"))
            .len(),
        1
    );
}
