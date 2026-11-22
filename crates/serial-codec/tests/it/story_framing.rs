//! Stories: streams that stay open for a long time.

use serial_codec::framing::*;
use std::time::Instant;

fn start_end(max: Option<usize>) -> Box<dyn Framer> {
    FramerConfig::StartEnd {
        start_delimiter: "<<".into(),
        end_delimiter: ">>".into(),
        include_start_delimiter: false,
        include_end_delimiter: false,
        max_buffer_size: max,
    }
    .build()
    .unwrap()
}

fn regex(pattern: &str) -> Box<dyn Framer> {
    FramerConfig::Regex {
        regex: pattern.into(),
        max_buffer_size: None,
        encoding: None,
    }
    .build()
    .unwrap()
}

/// One byte per read.
fn bytewise(f: &mut dyn Framer, data: &[u8]) -> Vec<Vec<u8>> {
    let now = Instant::now();
    let mut out = Vec::new();
    for b in data.chunks(1) {
        f.push(b, now, &mut out);
    }
    out
}

/// Given a device that talks for 256 KiB, one byte per read, without ever ending a frame
/// When the framer is startEnd (no start marker; then an open frame under a 4 MiB cap; then an
/// open frame at its default cap) and packetLength (a length no packet can reach)
/// Then the garbage gives no frame, the long open frame comes out whole with its end marker and
/// the next open one is flushed, the frame over the cap is dropped without the end marker
/// reviving it, and the endless packet is cut once at the cap with the rest outside a packet.
/// The work is linear: a framer that rescanned its buffer per byte would not finish.
#[test]
fn a_stream_that_never_ends_its_frame() {
    let noise = vec![b'x'; 1 << 18];
    let mut f = start_end(None);
    assert!(bytewise(&mut *f, &noise).is_empty());
    assert_eq!(bytewise(&mut *f, b"<<frame>>"), [b"frame"]);

    let mut f = start_end(Some(4 << 20));
    let stream = [b"junk<<", &noise[..], b">><<a>"].concat();
    assert_eq!(bytewise(&mut *f, &stream), std::slice::from_ref(&noise));
    assert_eq!(f.flush().as_deref(), Some(&b"<<a>"[..]));

    let mut f = start_end(None);
    let stream = [b"<<", &noise[..], b">>"].concat();
    assert!(bytewise(&mut *f, &stream).is_empty());
    assert_eq!(bytewise(&mut *f, b"<<next>>"), [b"next"]);

    let mut f = FramerConfig::PacketLength {
        delimiter: None,
        delimiter_bytes: None,
        packet_overhead: Some(usize::MAX),
        length_bytes: None,
        length_offset: None,
        max_len: Some(usize::MAX),
        max_buffer_size: Some(1024),
    }
    .build()
    .unwrap();
    let out = bytewise(&mut *f, &[&[0xaa, 0xff][..], &[0; 4096]].concat());
    assert_eq!(out.iter().map(Vec::len).collect::<Vec<_>>(), [1024]);
    assert_eq!(f.flush(), None);
}

/// Given a regex whose match is longer than the lookback, and one whose lookback would start
/// inside a two-byte character
/// When the text arrives in pieces
/// Then the long match is found once the text has doubled since the last full search, the other
/// as soon as its last byte arrives, and the text before each match is one frame
#[test]
fn a_regex_match_longer_than_the_lookback() {
    let mut f = regex("<-+>");
    let text = format!(
        "head<{}>tail{}",
        "-".repeat(3 * REGEX_LOOKBACK),
        ".".repeat(8 * REGEX_LOOKBACK)
    );
    assert_eq!(bytewise(&mut *f, text.as_bytes()), [b"head"]);

    let mut f = regex(&format!("жb{{{}}}!", REGEX_LOOKBACK - 1));
    let first = format!("aaaaж{}", "b".repeat(REGEX_LOOKBACK - 1));
    assert_eq!(first.len() - REGEX_LOOKBACK, 5, "the lookback splits ж");
    let mut out = Vec::new();
    f.push(first.as_bytes(), Instant::now(), &mut out);
    assert!(out.is_empty());
    f.push(b"!", Instant::now(), &mut out);
    assert_eq!(out, [b"aaaa"]);
}
