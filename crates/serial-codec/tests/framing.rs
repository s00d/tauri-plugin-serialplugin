//! Byte-stream framers through `FramerConfig`: chunking independence for every parser
//! type, bounded buffers on any input, packetLength / SLIP round trips, option
//! validation, and stories for the bounded-work rules (the timing models are in
//! `framing_model.rs`, the Node parity vectors in `framing_vectors.rs`).

use crate::common::*;
use serial_codec::framing::*;
use std::time::Instant;

fn run(cfg: &FramerConfig, parts: &[&[u8]]) -> Vec<Vec<u8>> {
    let mut f = cfg.build().unwrap();
    let t = Instant::now();
    let mut out = Vec::new();
    for p in parts {
        f.push(p, t, &mut out);
    }
    out.extend(f.flush());
    out
}

/// Bytes the framers below treat specially, plus text and broken UTF-8.
const ALPHA: &[u8] = b"\x00\x01\x02\x03\xAA\xAB\r\n;,END1abSE\xC0\xDB\xDC\xDD\xF9\xFF\xC3\xA9";

fn data(max: usize) -> impl Strategy<Value = Vec<u8>> {
    vec(
        prop_oneof![8 => select(ALPHA.to_vec()), 1 => any::<u8>()],
        0..max,
    )
}

fn marker() -> impl Strategy<Value = ByteSeq> {
    vec(select(b"\x00\x01\r\nab;SE".to_vec()), 1..=3).prop_map(ByteSeq::Bytes)
}

fn slip_options() -> impl Strategy<Value = SlipOptions> {
    prop_oneof![
        Just(SlipOptions::default()),
        Just(SlipOptions {
            start: Some(0xab),
            esc: 0xcd,
            end: 0xbc,
            esc_start: Some(0xac),
            esc_esc: 0xce,
            esc_end: 0xbd,
        }),
    ]
}

/// A config of every parser type whose output cannot depend on read chunking:
/// default buffer caps where a cap cuts at a read boundary (delimiter, regex, startEnd)
/// and regexes whose matches are shorter than the lookback.
fn config() -> impl Strategy<Value = FramerConfig> {
    prop_oneof![
        (marker(), any::<bool>()).prop_map(|(delimiter, include_delimiter)| {
            FramerConfig::Delimiter {
                delimiter,
                include_delimiter,
                max_buffer_size: None,
            }
        }),
        (
            proptest::option::of(Just(ByteSeq::Text("\r\n".into()))),
            any::<bool>()
        )
            .prop_map(|(delimiter, include_delimiter)| FramerConfig::Readline {
                delimiter,
                include_delimiter,
                max_buffer_size: None,
                encoding: None,
            }),
        (1usize..10).prop_map(|length| FramerConfig::ByteLength { length }),
        (1u64..100, 1usize..20).prop_map(|(interval_ms, m)| FramerConfig::InterByteTimeout {
            interval_ms,
            max_buffer_size: Some(m),
        }),
        packet_length_config(),
        marker().prop_map(|delimiter| FramerConfig::Ready { delimiter }),
        select(vec!["\n", "[\r\n]+", "\r\n|\n", "[;,]", "END[0-9];"]).prop_map(|r| {
            FramerConfig::Regex {
                regex: r.into(),
                max_buffer_size: None,
                encoding: None,
            }
        }),
        (marker(), marker(), any::<bool>(), any::<bool>()).prop_map(|(s, e, is, ie)| {
            FramerConfig::StartEnd {
                start_delimiter: s,
                end_delimiter: e,
                include_start_delimiter: is,
                include_end_delimiter: ie,
                max_buffer_size: None,
            }
        }),
        slip_options().prop_map(FramerConfig::Slip),
        proptest::option::of(Just(0u64))
            .prop_map(|max_delay_ms| FramerConfig::CcTalk { max_delay_ms }),
    ]
}

/// Valid packetLength options with a small buffer cap (its cap is per byte).
fn packet_length_config() -> impl Strategy<Value = FramerConfig> {
    (
        1usize..=2,
        1usize..=2,
        0usize..3,
        0usize..6,
        0usize..300,
        6usize..64,
    )
        .prop_map(
            |(db, lb, extra, overhead, max_len, max)| FramerConfig::PacketLength {
                delimiter: Some(if db == 1 {
                    OneOrMany::Many(vec![0xAA, 0x01])
                } else {
                    OneOrMany::One(0xAAAB)
                }),
                delimiter_bytes: Some(db),
                packet_overhead: Some(overhead),
                length_bytes: Some(lb),
                length_offset: Some(db + extra),
                max_len: Some(max_len),
                max_buffer_size: Some(max),
            },
        )
}

proptest! {
    #![proptest_config(cases(256))]

    /// One read, arbitrary reads and byte-by-byte reads give the same frames (the
    /// `ready` pass-through keeps read boundaries, so it gives the same bytes).
    #[test]
    fn chunking_never_changes_frames(cfg in config(), data in data(200), sizes in vec(1usize..16, 1..5)) {
        let whole = run(&cfg, &[&data]);
        let split = run(&cfg, &chunks(&data, &sizes));
        let bytewise = run(&cfg, &chunks(&data, &[1]));
        if matches!(cfg, FramerConfig::Ready { .. }) {
            prop_assert_eq!(split.concat(), whole.concat());
            prop_assert_eq!(bytewise.concat(), whole.concat());
        } else {
            prop_assert_eq!(&split, &whole);
            prop_assert_eq!(&bytewise, &whole);
        }
        prop_assert!(whole.iter().all(|f| !f.is_empty()), "an empty frame");
    }

    /// Any input with any small cap: no panic, no empty frame, and what stays buffered
    /// after the last read stays under the cap.
    #[test]
    fn buffers_stay_bounded(kind in 0u8..9, max in 1usize..20, len in 1usize..10, data in vec(any::<u8>(), 0..300), sizes in vec(1usize..40, 1..4)) {
        let m = Some(max);
        let cfg = match kind {
            0 => FramerConfig::Delimiter { delimiter: ByteSeq::Bytes(vec![0, 1]), include_delimiter: false, max_buffer_size: m },
            1 => FramerConfig::Regex { regex: "\n".into(), max_buffer_size: m, encoding: None },
            2 => FramerConfig::StartEnd { start_delimiter: ByteSeq::Bytes(vec![2]), end_delimiter: ByteSeq::Bytes(vec![3, 4]),
                include_start_delimiter: false, include_end_delimiter: false, max_buffer_size: m },
            3 => FramerConfig::InterByteTimeout { interval_ms: 5, max_buffer_size: m },
            4 => FramerConfig::ByteLength { length: len },
            5 => FramerConfig::PacketLength { delimiter: None, delimiter_bytes: None, packet_overhead: Some(usize::MAX),
                length_bytes: None, length_offset: None, max_len: Some(usize::MAX), max_buffer_size: Some(max + 2) },
            6 => FramerConfig::Slip(SlipOptions::default()),
            7 => FramerConfig::CcTalk { max_delay_ms: None },
            _ => FramerConfig::Ready { delimiter: ByteSeq::Bytes(vec![5, 6]) },
        };
        let mut f = cfg.build().unwrap();
        prop_assert_eq!(f.flush(), None, "fresh framer");
        let t = Instant::now();
        let mut out = Vec::new();
        let parts = chunks(&data, &sizes);
        for p in &parts {
            f.push(p, t, &mut out);
        }
        prop_assert!(out.iter().all(|f| !f.is_empty()));
        let rest = f.flush().map_or(0, |r| r.len());
        let last = parts.last().map_or(0, |p| p.len());
        let bound = match kind {
            1 => max + 3,            // plus an incomplete UTF-8 sequence
            2 => max + last,         // an open frame is dropped once it reaches the cap
            4 => len,
            5 => max + 2,
            6 => DEFAULT_MAX_BUFFER,
            7 | 8 => 1,
            _ => max,
        };
        prop_assert!(rest < bound, "{} bytes left, bound {}", rest, bound);
    }

    /// packetLength: delimited packets with a little-endian length (any width/offset)
    /// come out whole; flag-free garbage between them is skipped.
    #[test]
    fn packet_length_round_trip(
        db in 1usize..=2, lb in 1usize..=2, extra in 0usize..3, trailer in 0usize..3,
        packets in vec((0usize..60, vec(4u8..0xA0, 0..4)), 0..6),
    ) {
        let offset = db + extra;
        let overhead = offset + lb + trailer;
        let cfg = FramerConfig::PacketLength {
            delimiter: Some(OneOrMany::One(if db == 1 { 0xAA } else { 0xAAAB })),
            delimiter_bytes: Some(db), packet_overhead: Some(overhead), length_bytes: Some(lb),
            length_offset: Some(offset), max_len: Some(1000), max_buffer_size: None,
        };
        let mut stream = Vec::new();
        let mut want = Vec::new();
        for (len, junk) in &packets {
            stream.extend(junk);
            let mut p = if db == 1 { vec![0xAA] } else { vec![0xAA, 0xAB] };
            p.resize(offset, 0x11);
            p.extend(&len.to_le_bytes()[..lb]);
            p.resize(len + overhead, 0x22);
            stream.extend(&p);
            want.push(p);
        }
        prop_assert_eq!(run(&cfg, &[&stream]), want);
    }

    /// packetLength options validate exactly as documented.
    #[test]
    fn packet_length_options_validate(
        delims in proptest::option::of(vec(any::<u32>(), 0..3)),
        db in proptest::option::of(0usize..6), lb in proptest::option::of(0usize..10),
        offset in proptest::option::of(prop_oneof![0usize..20, Just(usize::MAX)]),
        max in proptest::option::of(0usize..24),
    ) {
        let cfg = FramerConfig::PacketLength {
            delimiter: delims.clone().map(OneOrMany::Many), delimiter_bytes: db, packet_overhead: None,
            length_bytes: lb, length_offset: offset, max_len: None, max_buffer_size: max,
        };
        let (d, b, l, o, m) = (delims.map_or(1, |v| v.len()), db.unwrap_or(1), lb.unwrap_or(1),
            offset.unwrap_or(1), max.unwrap_or(DEFAULT_MAX_BUFFER));
        let ok = m > 0 && d > 0 && (1..=4).contains(&b) && (1..=8).contains(&l) && o >= b
            && o.checked_add(l).is_some_and(|end| end <= m);
        prop_assert_eq!(cfg.build().is_ok(), ok);
    }

    /// SLIP: encoded packets decode back (empty ones vanish), with and without the
    /// bluetooth quirk and the optional START byte.
    #[test]
    fn slip_round_trip(o in slip_options(), quirk in any::<bool>(), packets in vec(vec(any::<u8>(), 0..20), 0..6), sizes in vec(1usize..16, 1..4)) {
        let stream: Vec<u8> = packets.iter().flat_map(|p| slip_encode(p, &o, quirk)).collect();
        let want: Vec<Vec<u8>> = packets.into_iter().filter(|p| !p.is_empty()).collect();
        prop_assert_eq!(run(&FramerConfig::Slip(o), &chunks(&stream, &sizes)), want);
    }
}

// ------------------------------------------------------------------ stories

fn bytewise(cfg: FramerConfig, data: &[u8]) -> (Vec<Vec<u8>>, Box<dyn Framer>) {
    let mut f = cfg.build().unwrap();
    let t = Instant::now();
    let mut out = Vec::new();
    for b in data.chunks(1) {
        f.push(b, t, &mut out);
    }
    (out, f)
}

fn start_end(start: &str, end: &str, max: Option<usize>) -> FramerConfig {
    FramerConfig::StartEnd {
        start_delimiter: start.into(),
        end_delimiter: end.into(),
        include_start_delimiter: false,
        include_end_delimiter: false,
        max_buffer_size: max,
    }
}

/// Given 256 KiB of input without markers, pushed byte by byte into startEnd
/// When it has all arrived
/// Then no frame came out (the old framer rescanned its buffer per byte: 15 s)
#[test]
fn start_end_garbage_is_linear() {
    let (out, _) = bytewise(start_end("<<", ">>", None), &vec![b'x'; 1 << 18]);
    assert!(out.is_empty());
}

/// Given a 256 KiB open startEnd frame pushed byte by byte under a 4 MiB cap
/// When its end marker and a new open frame follow
/// Then the long frame comes out whole and the open one is flushed
#[test]
fn start_end_long_open_frame() {
    let mut data = b"junk<<".to_vec();
    data.extend(vec![b'x'; 1 << 18]);
    data.extend(b">><<a>");
    let (out, mut f) = bytewise(start_end("<<", ">>", Some(4 << 20)), &data);
    assert_eq!(out, vec![vec![b'x'; 1 << 18]]);
    assert_eq!(f.flush(), Some(b"<<a>".to_vec()));
}

/// Given a startEnd frame opened at the buffer start that fills the cap without an end
/// When the end marker arrives
/// Then nothing comes out: there was no later start marker to keep
#[test]
fn start_end_overlong_frame_dropped() {
    let mut f = start_end("S", "E", Some(4)).build().unwrap();
    let mut out = Vec::new();
    f.push(b"Sabc", Instant::now(), &mut out);
    f.push(b"E", Instant::now(), &mut out);
    assert!(out.is_empty());
}

/// Given start marker `AB` and end marker `BC`
/// When `AB` then `C` arrive, then `xBC`
/// Then the `B` of the start marker is no end marker: the frame is `Cx`
#[test]
fn start_end_end_marker_never_overlaps_the_start_marker() {
    assert_eq!(
        run(&start_end("AB", "BC", None), &[b"AB", b"C", b"xBC"]),
        vec![b"Cx".to_vec()]
    );
}

/// Given a regex match longer than the lookback, fed byte by byte
/// When the text has doubled since the last full scan
/// Then the match is found and the text before it is a frame
#[test]
fn regex_long_match_found_by_doubling_rescan() {
    let long = format!("<{}>", "-".repeat(3 * REGEX_LOOKBACK));
    let mut data = b"head".to_vec();
    data.extend(long.as_bytes());
    data.extend(b"tail");
    data.extend(vec![b'.'; 8 * REGEX_LOOKBACK]);
    let cfg = FramerConfig::Regex {
        regex: "<-+>".into(),
        max_buffer_size: None,
        encoding: None,
    };
    assert_eq!(bytewise(cfg, &data).0, vec![b"head".to_vec()]);
}

/// Given a regex whose lookback start falls inside a multi-byte character
/// When the last byte of the match arrives
/// Then the search backs off to the character start and finds the match
#[test]
fn regex_lookback_inside_a_char() {
    let cfg = FramerConfig::Regex {
        regex: "жb{31}!".into(),
        max_buffer_size: None,
        encoding: None,
    };
    let first = format!("aaaaж{}", "b".repeat(31));
    assert_eq!(first.len(), 37);
    let mut f = cfg.build().unwrap();
    let mut out = Vec::new();
    f.push(first.as_bytes(), Instant::now(), &mut out);
    assert!(out.is_empty());
    f.push(b"!", Instant::now(), &mut out);
    assert_eq!(out, vec![b"aaaa".to_vec()]);
}

/// Given packetLength with an overhead no packet can reach and a 1 KiB cap
/// When a 4 KiB packet arrives
/// Then it is cut at the cap and the bytes after the cut are outside a packet
#[test]
fn packet_length_endless_packet_is_cut() {
    let cfg = FramerConfig::PacketLength {
        delimiter: None,
        delimiter_bytes: None,
        packet_overhead: Some(usize::MAX),
        length_bytes: None,
        length_offset: None,
        max_len: Some(usize::MAX),
        max_buffer_size: Some(1024),
    };
    let mut data = vec![0xaa, 0xff];
    data.extend(vec![0u8; 4096]);
    let mut f = cfg.build().unwrap();
    let mut out = Vec::new();
    f.push(&data, Instant::now(), &mut out);
    assert_eq!(out.iter().map(Vec::len).collect::<Vec<_>>(), vec![1024]);
    assert!(f.flush().is_none());
}

/// Given parser options as JSON from the frontend
/// When an option name has a typo
/// Then the config is rejected, while the Node names and aliases are accepted
#[test]
fn option_names() {
    use serde_json::json;
    let parse =
        |v: serde_json::Value| serde_json::from_value::<FramerConfig>(v).map(|c| c.build().is_ok());
    assert!(parse(json!({"type": "readline", "delimter": "\r\n"})).is_err());
    assert!(parse(json!({"type": "byteLength", "length": 4, "lenght": 4})).is_err());
    assert!(parse(json!({"type": "slip", "ESCAPE": 1})).is_err());
    assert!(parse(json!({"type": "packetLength", "maxLength": 4})).is_err());
    for ok in [
        json!({"type": "interByteTimeout", "interval": 5}),
        json!({"type": "ccTalk", "maxDelayBetweenBytesMs": 5}),
        json!({"type": "slip", "ESC_END": 1, "escEsc": 2}),
        json!({"type": "readline", "encoding": "utf8"}),
    ] {
        assert_eq!(parse(ok.clone()).ok(), Some(true), "{ok}");
    }
}
