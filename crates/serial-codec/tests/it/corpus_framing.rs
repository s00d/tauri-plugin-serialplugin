//! Corpus: the Node.js `@serialport/parser-*` test suites, one test per package. Case titles are
//! the Node `it(...)` titles; cases past a `// Not in Node` line pin a documented difference.
//!
//! Node's `Buffer.from(string)` is UTF-8, so a Node literal like `'\xaa'` is the two bytes
//! `c2 aa`; such inputs are written as Rust `&str` here.

use serde_json::{json, Value};
use serial_codec::framing::*;
use std::time::{Duration, Instant};

fn build(title: &str, v: Value) -> Box<dyn Framer> {
    serde_json::from_value::<FramerConfig>(v)
        .unwrap_or_else(|e| panic!("{title}: {e}"))
        .build()
        .unwrap_or_else(|e| panic!("{title}: {e}"))
}

fn frames<T: AsRef<[u8]>>(v: &[T]) -> Vec<Vec<u8>> {
    v.iter().map(|f| f.as_ref().to_vec()).collect()
}

/// `parser.write(chunk)` for each chunk gives `want`, then `parser.end()` gives `end`.
fn case_end<T: AsRef<[u8]>>(
    title: &str,
    cfg: Value,
    writes: &[&[u8]],
    want: &[T],
    end: Option<&[u8]>,
) {
    let mut f = build(title, cfg);
    let now = Instant::now();
    let mut out = Vec::new();
    for c in writes {
        f.push(c, now, &mut out);
    }
    assert_eq!(out, frames(want), "{title}");
    assert_eq!(f.flush().as_deref(), end, "{title}: end");
}

/// The frames the writes give; the Node test does not end the stream.
fn case<T: AsRef<[u8]>>(title: &str, cfg: Value, writes: &[&[u8]], want: &[T]) {
    let mut f = build(title, cfg);
    let now = Instant::now();
    let mut out = Vec::new();
    for c in writes {
        f.push(c, now, &mut out);
    }
    assert_eq!(out, frames(want), "{title}");
}

/// Steps of one parser at given times: a write or (no bytes) a timer tick.
fn timed(title: &str, cfg: Value, steps: &[(u64, &[u8])], want: &[&[u8]]) {
    let mut f = build(title, cfg);
    let start = Instant::now();
    let mut out = Vec::new();
    for (ms, data) in steps {
        let now = start + Duration::from_millis(*ms);
        if data.is_empty() {
            f.poll(now, &mut out);
        } else {
            f.push(data, now, &mut out);
        }
    }
    assert_eq!(out, frames(want), "{title}");
}

/// Node `assert.throws(() => new XParser(opts))`: a bad JSON shape or failed validation.
fn throws(title: &str, options: &[Value]) {
    for v in options {
        let built = serde_json::from_value::<FramerConfig>(v.clone()).map(|c| c.build().is_ok());
        assert!(!built.unwrap_or(false), "{title}: {v}");
    }
}

/// Options that build (Node: the constructor does not throw). A Node Buffer is `ByteSeq::Bytes`
/// from Rust and a byte array in JSON.
fn allows(title: &str, options: &[Value]) {
    for v in options {
        build(title, v.clone());
    }
}

const NONE: &[&[u8]] = &[];

#[test]
fn byte_length() {
    let p = |n: usize| json!({"type": "byteLength", "length": n});
    case(
        "emits data events every 8 bytes",
        p(8),
        &[b"Robots are so freaking cool!"],
        &[b"Robots a", b"re so fr", b"eaking c"],
    );
    throws(
        "throws when not provided with a length",
        &[Value::Null, json!({"type": "byteLength"})],
    );
    throws("throws when length is zero", &[p(0)]);
    throws(
        "throws when called with a non numeric length",
        &[json!({"type": "byteLength", "length": "foop"})],
    );
    case(
        "continues looking for bytes in additional writes",
        p(4),
        &[b"ab", b"cd"],
        &[b"abcd"],
    );
    case_end(
        "flushes remaining data when the stream ends",
        p(4),
        &[b"12"],
        NONE,
        Some(b"12"),
    );
}

#[test]
fn cctalk() {
    let p = || json!({"type": "ccTalk"});
    allows("constructs", &[p()]);
    case(
        "emits data for a default length message",
        p(),
        &[&[2, 0, 1, 254, 217]],
        &[[2, 0, 1, 254, 217]],
    );
    case(
        "emits data for a 7 byte length message",
        p(),
        &[&[2, 2, 1, 254, 1, 1, 217]],
        &[[2, 2, 1, 254, 1, 1, 217]],
    );
    let (long, short): (&[u8], &[u8]) = (&[2, 2, 1, 254, 1, 1, 217], &[2, 0, 1, 254, 217]);
    case(
        "parses multiple length messages",
        p(),
        &[&[2, 2, 1], &[254, 1, 1], &[217, 2], &[0, 1, 254, 217]],
        &[long, short],
    );
    case(
        "parses a long message",
        p(),
        &[&[
            2, 2, 1, 254, 1, 1, 217, 2, 0, 1, 254, 217, 2, 2, 1, 251, 1, 1, 217, 2, 2, 1, 252, 1,
            1, 217, 2, 0, 1, 253, 217,
        ]],
        &[
            long,
            short,
            &[2, 2, 1, 251, 1, 1, 217],
            &[2, 2, 1, 252, 1, 1, 217],
            &[2, 0, 1, 253, 217],
        ],
    );
    timed(
        "resets incomplete message after timeout",
        json!({"type": "ccTalk", "maxDelayMs": 50}),
        &[(0, &[2, 2, 1]), (51, long)],
        &[long],
    );
    timed(
        "disabled message timeout",
        json!({"type": "ccTalk", "maxDelayMs": 0}),
        &[(0, &[2, 2, 1]), (100, &[254, 1, 1, 217])],
        &[long],
    );
    timed(
        "the Node option name is accepted",
        json!({"type": "ccTalk", "maxDelayBetweenBytesMs": 0}),
        &[(0, &[2, 0]), (1000, &[1, 254, 217])],
        &[short],
    );
    case_end(
        "a partial frame is dropped at the end",
        p(),
        &[&[2, 2, 1]],
        NONE,
        None,
    );
}

#[test]
fn delimiter() {
    let p = |d: Value| json!({"type": "delimiter", "delimiter": d});
    let lines: [&[u8]; 4] = [
        b"I love robots\nEach ",
        b"and Every One\n",
        b"\n",
        b"even you!",
    ];
    case(
        "transforms data to strings split on a delimiter",
        p(json!("\n")),
        &lines,
        &[&b"I love robots"[..], b"Each and Every One"],
    );
    case(
        "includes delimiter when includeDelimiter is true",
        json!({"type": "delimiter", "delimiter": "\n", "includeDelimiter": true}),
        &lines,
        &[&b"I love robots\n"[..], b"Each and Every One\n", b"\n"],
    );
    case_end(
        "flushes remaining data when the stream ends",
        p(json!([0])),
        &[&[1]],
        NONE,
        Some(&[1]),
    );
    throws(
        "throws when not provided with a delimiter",
        &[json!({"type": "delimiter"}), Value::Null],
    );
    throws(
        "throws when called with a 0 length delimiter",
        &[p(json!([])), p(json!(""))],
    );
    allows(
        "allows setting of the delimiter with a string",
        &[p(json!("string"))],
    );
    allows(
        "allows setting of the delimiter with a buffer / an array of bytes",
        &[p(json!([1]))],
    );
    assert_eq!(
        serde_json::from_value::<FramerConfig>(p(json!([1]))).unwrap(),
        FramerConfig::Delimiter {
            delimiter: ByteSeq::from(&[1u8][..]),
            include_delimiter: false,
            max_buffer_size: None,
        }
    );
    let moteino: [&[u8]; 3] = [b"This could be", b"binary data", b"sent from a Moteino"];
    case(
        "emits data events every time it meets 00x 00x",
        p(json!([0, 0])),
        &[b"This could be\0\0binary data\0\0sent from a Moteino\0\0"],
        &moteino,
    );
    case(
        "accepts single byte delimiter",
        p(json!([0])),
        &[b"This could be\0binary data\0sent from a Moteino\0"],
        &moteino,
    );
    case(
        "Works when buffer starts with delimiter",
        p(json!([0])),
        &[b"\0Hello\0World\0"],
        &[b"Hello", b"World"],
    );
    case(
        "should only emit if delimiters are strictly in row",
        p(json!([0, 1])),
        &[b"\0Hello\x01World\0\0\x01"],
        &[b"\0Hello\x01World\0"],
    );
    case(
        "continues looking for delimiters in the next buffers",
        p(json!([0, 0])),
        &[
            b"This could be\0\0binary ",
            b"data\0\0sent from a Moteino\0\0",
        ],
        &moteino,
    );
    case(
        "works if a multibyte delimiter crosses a chunk boundary",
        p(json!([0, 1])),
        &[&[1, 2, 3, 0], &[1, 2, 3, 0], &[1]],
        &[&[1, 2, 3][..], &[2, 3]],
    );
    // Not in Node: bounded buffer.
    let bounded = |n: usize| json!({"type": "delimiter", "delimiter": "\n", "maxBufferSize": n});
    case(
        "an overlong line",
        bounded(4),
        &[b"abcdef", b"\n"],
        &[&b"abcd"[..], b"ef"],
    );
    throws("no buffer", &[bounded(0)]);
}

#[test]
fn readline() {
    let p = |d: Value| json!({"type": "readline", "delimiter": d});
    case_end(
        "transforms data to strings split on a delimiter",
        json!({"type": "readline"}),
        &[b"I love robots\nEach ", b"and Every One\n", b"even you!"],
        &[&b"I love robots"[..], b"Each and Every One"],
        Some(b"even you!"),
    );
    for (title, d) in [
        ("allows setting of the delimiter with a string", json!("a")),
        (
            "allows setting of the delimiter with a buffer / an array of bytes",
            json!([97]),
        ),
    ] {
        case(title, p(d), &[b"how are youa"], &[&b"how "[..], b"re you"]);
    }
    // Node renders '61', '62' with `encoding: 'hex'`; frames are bytes here.
    case(
        "allows setting of encoding",
        json!({"type": "readline", "encoding": "hex"}),
        &[b"a\nb\n"],
        &[b"a", b"b"],
    );
    // Node decodes a string delimiter with `encoding`; here a string is UTF-8, so give bytes.
    case(
        "encoding should be reflected in a string delimiter",
        json!({"type": "readline", "delimiter": [255], "encoding": "hex"}),
        &[&[0, 255, 1, 255]],
        &[[0], [1]],
    );
    throws(
        "throws when called with a 0 length delimiter",
        &[p(json!([])), p(json!(""))],
    );
    case(
        "doesn't emit empty data events",
        p(json!("a")),
        &[b"aFa"],
        &[b"F"],
    );
}

#[test]
fn ready() {
    let p = |d: Value| json!({"type": "ready", "delimiter": d});
    case(
        "emits data received after the ready data",
        p(json!("\n")),
        &[b"which will you get?", b"garbage\ngold", b"just for you"],
        &[&b"gold"[..], b"just for you"],
    );
    case(
        "emits the ready event before the data event",
        p(json!("!")),
        &[b"!hi"],
        &[b"hi"],
    );
    // `ready` is internal here: it shows as data passing through.
    case(
        "has a ready property",
        p(json!("\n")),
        &[b"not the new line", b"this is the \n", b"x"],
        &[b"x"],
    );
    throws(
        "throws when not provided with a delimiter",
        &[Value::Null, json!({"type": "ready"})],
    );
    throws(
        "throws when called with a 0 length delimiter",
        &[p(json!([])), p(json!(""))],
    );
    allows(
        "allows setting of the delimiter with a string",
        &[p(json!("string"))],
    );
    allows(
        "allows setting of the delimiter with a buffer / an array of bytes",
        &[p(json!([1]))],
    );
    case(
        "allows receiving the delimiter over small writes",
        p(json!("READY")),
        &[b"bad data then REA", b"D", b"Y", b"!!!!!!"],
        &[b"!!!!!!"],
    );
    // Not in Node: after `R`,`R` Node resets without re-checking the second `R`.
    case("a false start", p(json!("READY")), &[b"RREADY!"], &[b"!"]);
    case(
        "a false start in pieces",
        p(json!("READY")),
        &[b"xRR", b"EA", b"DY!"],
        &[b"!"],
    );
    case(
        "a repeated first byte",
        p(json!("aab")),
        &[b"aaab", b"z"],
        &[b"z"],
    );
    case_end(
        "nothing before ready is flushed",
        p(json!("READY")),
        &[b"REA"],
        NONE,
        None,
    );
}

#[test]
fn regex() {
    let p = |r: &str| json!({"type": "regex", "regex": r});
    case(
        "transforms data to strings split on either carriage return or new line",
        p("[\\r\\n]+"),
        &[
            b"I love robots\r\nEach ",
            b"and Every One\r",
            b"even you!",
            b"\nThe angry red robot",
        ],
        &[&b"I love robots"[..], b"Each and Every One", b"even you!"],
    );
    case_end(
        "flushes remaining data when the stream ends",
        p("\\n"),
        &[&[1]],
        NONE,
        Some(&[1]),
    );
    throws(
        "throws when not provided with a regex",
        &[json!({"type": "regex"})],
    );
    throws(
        "throws when called with an invalid regex expression",
        &[p("\\")],
    );
    // Node also turns a Buffer pattern into its string; here the pattern is a string.
    case(
        "allows setting of the regex with a regex string / a buffer",
        p("a|b"),
        &[b"bhow are youa"],
        &[&b"how "[..], b"re you"],
    );
    case(
        "allows setting of encoding",
        json!({"type": "regex", "regex": "\\r", "encoding": "hex"}),
        &[b"a\rb\r"],
        &[b"a", b"b"],
    );
    case(
        "Works when buffer starts with regex",
        p("\\r"),
        &[b"\rHello\rWorld\r"],
        &[b"Hello", b"World"],
    );
    case(
        "should match unicode in buffer string",
        p("\\r\\n|\\n"),
        &["\u{000a}Hello\u{000a}World\u{000d}\u{000a}!".as_bytes()],
        &[b"Hello", b"World"],
    );
    case(
        "continues looking for regexs in the next buffers",
        p("\\r\\n|\\n"),
        &[
            b"This could be\na poem ",
            b"or prose\r\nsent from a robot\r\n",
        ],
        &[
            &b"This could be"[..],
            b"a poem or prose",
            b"sent from a robot",
        ],
    );
    case(
        "doesn't emit empty data events",
        p("a|b"),
        &[b"abaFab"],
        &[b"F"],
    );
    // Not in Node: it decodes each chunk alone and would emit "x\u{FFFD}\u{FFFD}".
    case(
        "a split UTF-8 character",
        p("\\n"),
        &[&[b'x', 0xc3], &[0xa9, b'\n']],
        &["xé"],
    );
    case(
        "overlong text",
        json!({"type": "regex", "regex": "\\n", "maxBufferSize": 4}),
        &[b"ab", b"cd", b"e\n"],
        &[&b"abcd"[..], b"e"],
    );
}

#[test]
fn slip_decoder() {
    let p = || json!({"type": "slip"});
    let custom = || {
        json!({
            "type": "slip",
            "START": 0xab, "ESC": 0xcd, "END": 0xbc,
            "ESC_START": 0xac, "ESC_ESC": 0xce, "ESC_END": 0xbd,
        })
    };
    case(
        "Decodes one-byte messages",
        p(),
        &[&[0x01, 0xc0], &[0x80, 0xc0], &[0xff, 0xc0], &[0xa5, 0xc0]],
        &[[0x01], [0x80], [0xff], [0xa5]],
    );
    case("No data event on zero-byte messages", p(), &[&[0xc0]], NONE);
    case(
        "Decodes escaped characters",
        p(),
        &[
            &[0x01, 0xc0],
            &[0xdb, 0xdc, 0xc0],
            &[0xdb, 0xdd, 0xc0],
            &[0xdc, 0xc0],
            &[0xdd, 0xc0],
            &[0xff, 0xc0],
        ],
        &[[0x01], [0xc0], [0xdb], [0xdc], [0xdd], [0xff]],
    );
    case(
        "Decodes escaped characters with custom escapes",
        custom(),
        &[
            &[0xab, 0x01, 0xbc],
            &[0xab, 0xcd, 0xbd, 0xbc],
            &[0xab, 0xcd, 0xce, 0xbc],
            &[0xab, 0xdc, 0xbc],
            &[0xab, 0xdd, 0xbc],
            &[0xab, 0xff, 0xbc],
            &[0xab, 0xcd, 0xac, 0xbc],
        ],
        &[[0x01], [0xbc], [0xcd], [0xdc], [0xdd], [0xff], [0xab]],
    );
    case(
        "Decodes invalid escape",
        custom(),
        &[&[0xab, 0x01, 0xcd, 0x02, 0xbc]],
        &[[0x01], [0x02]],
    );
    case(
        "Decodes data split across multiple chunks",
        custom(),
        &[&[0xab, 0x01], &[0xcd, 0xbd, 0xbc]],
        &[[0x01, 0xbc]],
    );
    case(
        "Decodes data split across multiple chunks on escape",
        custom(),
        &[&[0xab, 0x01, 0xcd], &[0xbd, 0xbc]],
        &[[0x01, 0xbc]],
    );
    case(
        "Data before START is dropped",
        json!({"type": "slip", "START": 0xab, "END": 0xbc}),
        &[&[0x01, 0x02, 0xdb], &[0xab, 0x03, 0x04, 0xbc]],
        &[[0x03, 0x04]],
    );
    case_end(
        "Flushes pending data on stream end",
        custom(),
        &[&[0xab, 0x01, 0xcd]],
        NONE,
        Some(&[0x01]),
    );
    // Not in Node: camelCase names, a bounded buffer.
    case(
        "camelCase option names",
        json!({"type": "slip", "start": 0xab, "end": 0xbc, "escEsc": 0xdd, "ESC_END": 0xdc}),
        &[&[0x01, 0xab, 0x02, 0xbc]],
        &[[0x02]],
    );
    case_end(
        "an endless packet",
        p(),
        &[&vec![1u8; DEFAULT_MAX_BUFFER + 1]],
        &[vec![1u8; DEFAULT_MAX_BUFFER]],
        Some(&[1]),
    );
}

#[test]
fn slip_encoder() {
    let default = SlipOptions::default();
    let custom = SlipOptions {
        start: Some(0xab),
        esc: 0xcd,
        end: 0xbc,
        esc_start: Some(0xac),
        esc_esc: 0xce,
        esc_end: 0xbd,
    };
    let case = |title: &str, o: &SlipOptions, quirk: bool, packets: &[&[u8]], want: &[&[u8]]| {
        let out: Vec<Vec<u8>> = packets.iter().map(|p| slip_encode(p, o, quirk)).collect();
        assert_eq!(out, frames(want), "{title}");
    };
    let one_byte: [&[u8]; 4] = [&[0x01], &[0x80], &[0xff], &[0xa5]];
    let escapes: [&[u8]; 6] = [&[0x01], &[0xc0], &[0xdb], &[0xdc], &[0xdd], &[0xff]];
    case(
        "Adds one delimiter to one-byte messages",
        &default,
        false,
        &one_byte,
        &[&[0x01, 0xc0], &[0x80, 0xc0], &[0xff, 0xc0], &[0xa5, 0xc0]],
    );
    case(
        "Adds two delimiters to one-byte messages with the bluetooth quirk",
        &default,
        true,
        &one_byte,
        &[
            &[0xc0, 0x01, 0xc0],
            &[0xc0, 0x80, 0xc0],
            &[0xc0, 0xff, 0xc0],
            &[0xc0, 0xa5, 0xc0],
        ],
    );
    case(
        "Adds one delimiter to zero-byte messages",
        &default,
        false,
        &[&[]],
        &[&[0xc0]],
    );
    case(
        "Does nothing with zero-byte messages with the bluetooth quirk",
        &default,
        true,
        &[&[], &[]],
        &[&[], &[]],
    );
    case(
        "Escapes characters",
        &default,
        false,
        &escapes,
        &[
            &[0x01, 0xc0],
            &[0xdb, 0xdc, 0xc0],
            &[0xdb, 0xdd, 0xc0],
            &[0xdc, 0xc0],
            &[0xdd, 0xc0],
            &[0xff, 0xc0],
        ],
    );
    case(
        "Escapes characters with custom escapes",
        &custom,
        false,
        &[
            &[0x01],
            &[0xbc],
            &[0xcd],
            &[0xdc],
            &[0xdd],
            &[0xff],
            &[0xab],
        ],
        &[
            &[0xab, 0x01, 0xbc],
            &[0xab, 0xcd, 0xbd, 0xbc],
            &[0xab, 0xcd, 0xce, 0xbc],
            &[0xab, 0xdc, 0xbc],
            &[0xab, 0xdd, 0xbc],
            &[0xab, 0xff, 0xbc],
            &[0xab, 0xcd, 0xac, 0xbc],
        ],
    );
    case(
        "Escapes characters with the bluetooth quirk",
        &default,
        true,
        &escapes,
        &[
            &[0xc0, 0x01, 0xc0],
            &[0xc0, 0xdb, 0xdc, 0xc0],
            &[0xc0, 0xdb, 0xdd, 0xc0],
            &[0xc0, 0xdc, 0xc0],
            &[0xc0, 0xdd, 0xc0],
            &[0xc0, 0xff, 0xc0],
        ],
    );
    for o in [default, custom] {
        let data: Vec<u8> = (0..=255).collect();
        let mut out = Vec::new();
        SlipDecoder::new(o.clone()).push(&slip_encode(&data, &o, true), Instant::now(), &mut out);
        assert_eq!(out, [data], "encoder and decoder round trip");
    }
}

#[test]
fn start_end() {
    let input: [&[u8]; 4] = [
        b"\x02I love robots\x03\x02Each ",
        b"and Every One\x03",
        b"\x02",
        b"even you!",
    ];
    let p = |start: bool, end: bool| {
        json!({
            "type": "startEnd", "startDelimiter": "\u{2}", "endDelimiter": "\u{3}",
            "includeStartDelimiter": start, "includeEndDelimiter": end,
        })
    };
    let markers =
        |s: Value, e: Value| json!({"type": "startEnd", "startDelimiter": s, "endDelimiter": e});
    let want: [(&str, bool, bool, [&[u8]; 2]); 4] = [
        (
            "transforms data to strings split on startDelimiter & endDelimiter",
            false,
            false,
            [b"I love robots", b"Each and Every One"],
        ),
        (
            "includes startDelimiter when includeStartDelimiter is true",
            true,
            false,
            [b"\x02I love robots", b"\x02Each and Every One"],
        ),
        (
            "includes endDelimiter when includeEndDelimiter is true",
            false,
            true,
            [b"I love robots\x03", b"Each and Every One\x03"],
        ),
        (
            "includes both delimiters when both are true",
            true,
            true,
            [b"\x02I love robots\x03", b"\x02Each and Every One\x03"],
        ),
    ];
    for (title, start, end, want) in want {
        case(title, p(start, end), &input, &want);
    }
    case_end(
        "flushes remaining data when the stream ends",
        p(false, false),
        &[&[1]],
        NONE,
        Some(&[1]),
    );
    throws(
        "throws when not provided with a startDelimiter / an endDelimiter",
        &[
            json!({"type": "startEnd", "endDelimiter": "\u{3}"}),
            json!({"type": "startEnd", "startDelimiter": "\u{2}"}),
        ],
    );
    throws(
        "throws when called with a 0 length startDelimiter / endDelimiter",
        &[
            markers(json!([]), json!("\u{3}")),
            markers(json!(""), json!("\u{3}")),
            markers(json!("\u{2}"), json!([])),
            markers(json!("\u{2}"), json!("")),
        ],
    );
    allows(
        "allows setting of the delimiters with strings / buffers / arrays of bytes",
        &[
            markers(json!("string"), json!("string")),
            markers(json!([1]), json!([1])),
        ],
    );
    case(
        "Works when buffer starts with startDelimiter & endDelimiter",
        p(false, false),
        &[b"\x02\x03\x02Hello\x03\x02World\x03"],
        &[b"Hello", b"World"],
    );
    case(
        "continues looking for delimiters in the next buffers",
        p(false, false),
        &[
            b"\x02This could be\x03\x02binary ",
            b"data\x03\x02sent from a Moteino\x03",
        ],
        &[
            &b"This could be"[..],
            b"binary data",
            b"sent from a Moteino",
        ],
    );
    case(
        "works if a multibyte delimiter crosses a chunk boundary",
        markers(json!([7, 7]), json!([8, 8])),
        &[&[1, 2, 3, 7], &[7, 2, 3, 8], &[8]],
        &[[2, 3]],
    );
    // Not in Node: bounded buffer, a later start marker survives the cut.
    let bounded = || {
        json!({
            "type": "startEnd", "startDelimiter": "\u{2}", "endDelimiter": "\u{3}",
            "maxBufferSize": 8,
        })
    };
    case(
        "an overlong open frame",
        bounded(),
        &[b"\x02too long for it", b"\x02ok", b"\x03"],
        &[b"ok"],
    );
    case(
        "a later start marker",
        bounded(),
        &[b"\x02junk\x02ok", b"\x03"],
        &[b"ok"],
    );
}

#[test]
fn packet_length() {
    let p = || json!({"type": "packetLength"});
    let cat = |head: &[u8], tail: &str| [head, tail.as_bytes()].concat();
    let one = "I love robots";
    let two = "Each and Every One\n";
    let (end, you) = (b"and Every One\n", b"even you!");
    case(
        "transforms data to packets of correct length starting with delimiter",
        p(),
        &[
            "\u{aa}\u{0d}I love robots\u{aa}\u{13}Each ".as_bytes(),
            end,
            &[0xaa, 0x24],
            you,
        ],
        &[cat(&[0xaa, 0x0d], one), cat(&[0xaa, 0x13], two)],
    );
    case(
        "when length is offset",
        json!({"type": "packetLength", "lengthOffset": 2, "packetOverhead": 3}),
        &[
            "\u{aa}\u{01}\u{0d}I love robots\u{aa}\u{02}\u{13}Each ".as_bytes(),
            end,
            &[0xaa, 0x03, 0x24],
            you,
        ],
        &[cat(&[0xaa, 0x01, 0x0d], one), cat(&[0xaa, 0x02, 0x13], two)],
    );
    let wide = |more: Value| {
        let mut v = json!({"type": "packetLength", "lengthOffset": 2, "packetOverhead": 4, "lengthBytes": 2});
        v.as_object_mut()
            .unwrap()
            .extend(more.as_object().unwrap().clone());
        v
    };
    let head = "\u{aa}\u{01}\u{0d}\u{00}I love robots\u{aa}\u{02}\u{13}\u{00}Each ".as_bytes();
    let packets = [
        cat(&[0xaa, 0x01, 0x0d, 0x00], one),
        cat(&[0xaa, 0x02, 0x13, 0x00], two),
    ];
    case_end(
        "when length is offset and multibyte; flushes remaining data when the stream ends",
        wide(json!({})),
        &[head, end, &[0xaa, 0x03, 0x24, 0x00], you],
        &packets,
        Some(&cat(&[0xaa, 0x03, 0x24, 0x00], "even you!")),
    );
    case(
        "emits data early when invalid length encountered",
        wide(json!({"maxLen": 0x0d})),
        &[head, end, &[0xaa, 0x03, 0x09, 0x00], you],
        &[
            packets[0].clone(),
            vec![0xaa, 0x02, 0x13, 0x00],
            cat(&[0xaa, 0x03, 0x09, 0x00], "even you!"),
        ],
    );
    case(
        "works if a multibyte length crosses a chunk boundary",
        wide(json!({})),
        &[&head[..4], &head[4..], end, &[0xaa, 0x03, 0x24, 0x00], you],
        &packets,
    );
    case(
        "works with multiple delimiters",
        wide(json!({"delimiter": [0xaa, 0xbb]})),
        &[
            "\u{bb}\u{01}\u{0d}".as_bytes(),
            "\u{00}I love hobits\u{aa}\u{02}\u{13}\u{00}Each ".as_bytes(),
            end,
        ],
        &[
            cat(&[0xbb, 0x01, 0x0d, 0x00], "I love hobits"),
            packets[1].clone(),
        ],
    );
    let three = |d: Value| {
        json!({
            "type": "packetLength", "delimiter": d, "delimiterBytes": 3,
            "lengthOffset": 3, "packetOverhead": 4, "lengthBytes": 1,
        })
    };
    for (title, d, second) in [
        (
            "works with multibyte delimiters",
            json!([0xababba]),
            [0xab, 0xab, 0xba, 0x13],
        ),
        (
            "works with multiple multibyte delimiters",
            json!([0xababba, 0xbaddad]),
            [0xba, 0xdd, 0xad, 0x13],
        ),
    ] {
        case(
            title,
            three(d),
            &[
                &[0xab, 0xab, 0xba, 0x0d],
                b"I love hobits",
                &second,
                two.as_bytes(),
            ],
            &[
                cat(&[0xab, 0xab, 0xba, 0x0d], "I love hobits"),
                cat(&second, two),
            ],
        );
    }
    // Not in Node: it drops the newest byte on a mismatch and never finds these.
    for garbage in [&[0x00][..], &[0xab], &[0x01, 0x02, 0xab, 0xab]] {
        let packet = [0xab, 0xab, 0xba, 0x02, b'h', b'i'];
        case(
            "a multibyte delimiter after garbage",
            three(json!(0xababba)),
            &[garbage, &packet],
            &[packet],
        );
    }
    // Node would flush the partial delimiter window (byte-reversed).
    case_end(
        "flush drops bytes outside a packet",
        three(json!([0xababba])),
        &[&[0x01, 0xab, 0xab]],
        NONE,
        None,
    );
    case(
        "a header that outgrows len + overhead",
        json!({"type": "packetLength", "packetOverhead": 0}),
        &[&[0xaa, 0x00, 0x01, 0xaa, 0x01, 0x07]],
        &[[0xaa, 0x00], [0xaa, 0x01]],
    );
    throws(
        "unusable options",
        &[
            json!({"type": "packetLength", "delimiter": []}),
            json!({"type": "packetLength", "delimiterBytes": 0}),
            json!({"type": "packetLength", "delimiterBytes": 5}),
            json!({"type": "packetLength", "lengthBytes": 0}),
            json!({"type": "packetLength", "lengthBytes": 9}),
            json!({"type": "packetLength", "delimiterBytes": 2, "lengthOffset": 1}),
            json!({"type": "packetLength", "maxLength": 4}),
        ],
    );
}

#[test]
fn inter_byte_timeout() {
    let p = |i: Value| json!({"type": "interByteTimeout", "interval": i});
    timed(
        "emits data events after a pause of 30ms",
        p(json!(30)),
        &[
            (0, b"I love robots Each"),
            (0, b"and Every One"),
            (29, b""),
            (30, b""),
            (30, b"even you!"),
            (30, b"The angry red robot"),
            (60, b""),
        ],
        &[
            b"I love robots Eachand Every One",
            b"even you!The angry red robot",
        ],
    );
    throws(
        "throws when interval is not a number / is negative",
        &[
            p(json!(-20)),
            p(json!(f64::NAN)),
            p(json!("hello")),
            p(json!(0)),
            json!({"type": "interByteTimeout"}),
        ],
    );
    // NaN has no JSON form (serde_json writes null, which means "unset" here).
    for m in [json!(-20), json!("hello"), json!(0)] {
        throws(
            "throws when maxBufferSize is not a number / is negative",
            &[json!({"type": "interByteTimeout", "interval": 15, "maxBufferSize": m})],
        );
    }
    timed(
        "emits data events when buffer is full",
        json!({"type": "interByteTimeout", "interval": 15, "maxBufferSize": 2}),
        &[(0, &[1, 2, 3, 4, 5, 6]), (15, b"")],
        &[&[1, 2], &[3, 4], &[5, 6]],
    );
    case_end(
        "emits all buffered data when stream ends",
        p(json!(15)),
        &[b"Oh wow."],
        NONE,
        Some(b"Oh wow."),
    );
    case_end(
        "handles not having any buffered data when stream ends",
        p(json!(15)),
        &[b""],
        NONE,
        None,
    );
    // Not in Node: no timer, so a read may come late.
    timed(
        "a late read does not merge with a timed out packet",
        json!({"type": "interByteTimeout", "intervalMs": 10}),
        &[(0, b"a"), (50, b"b"), (60, b"")],
        &[b"a", b"b"],
    );
}
