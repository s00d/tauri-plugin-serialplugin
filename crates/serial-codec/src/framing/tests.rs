//! Ports of the Node.js `@serialport/parser-*` test suites (one `mod` per package,
//! test names follow the Node `it(...)` titles), followed by tests for the
//! deliberate differences listed in the module docs and chunk-boundary checks.
//!
//! Node's `Buffer.from(string)` is UTF-8, so a Node literal like `'\xaa'` is the two
//! bytes `c2 aa`; the ports keep that by writing such inputs as Rust `&str`.

use super::*;
use serde_json::{json, Value};

fn now() -> Instant {
    Instant::now()
}

fn build(v: Value) -> Box<dyn Framer> {
    serde_json::from_value::<FramerConfig>(v.clone())
        .unwrap_or_else(|e| panic!("{v}: {e}"))
        .build()
        .unwrap_or_else(|e| panic!("{v}: {e}"))
}

/// Node `assert.throws(() => new XParser(opts))`: bad JSON shape or failed validation.
fn rejects(v: Value) -> bool {
    serde_json::from_value::<FramerConfig>(v).map_or(true, |c| c.build().is_err())
}

/// `parser.write(chunk)` for each chunk, all at one instant.
fn write(f: &mut dyn Framer, chunks: &[&[u8]]) -> Vec<Vec<u8>> {
    let t = now();
    let mut out = Vec::new();
    for c in chunks {
        f.push(c, t, &mut out);
    }
    out
}

/// Writes followed by `parser.end()`.
fn write_end(f: &mut dyn Framer, chunks: &[&[u8]]) -> Vec<Vec<u8>> {
    let mut out = write(f, chunks);
    out.extend(f.flush());
    out
}

fn frames(v: &[&[u8]]) -> Vec<Vec<u8>> {
    v.iter().map(|f| f.to_vec()).collect()
}

/// Same frames whether the input arrives as the given chunks or one byte at a time.
fn same_bytewise(cfg: Value, chunks: &[&[u8]]) {
    let chunked = write_end(&mut *build(cfg.clone()), chunks);
    let all: Vec<u8> = chunks.concat();
    let bytes: Vec<&[u8]> = all.chunks(1).collect();
    let bytewise = write_end(&mut *build(cfg.clone()), &bytes);
    assert_eq!(chunked, bytewise, "{cfg}");
    assert!(!chunked.is_empty(), "{cfg}: test input produced no frames");
}

mod byte_length {
    use super::*;

    #[test]
    fn emits_data_events_every_8_bytes() {
        let mut p = build(json!({"type": "byteLength", "length": 8}));
        let out = write(&mut *p, &[b"Robots are so freaking cool!"]);
        assert_eq!(out, frames(&[b"Robots a", b"re so fr", b"eaking c"]));
    }

    #[test]
    fn throws_when_not_provided_with_a_length() {
        assert!(rejects(Value::Null));
        assert!(rejects(json!({"type": "byteLength"})));
    }

    #[test]
    fn throws_when_length_is_zero() {
        assert!(rejects(json!({"type": "byteLength", "length": 0})));
    }

    #[test]
    fn throws_when_called_with_a_non_numeric_length() {
        assert!(rejects(json!({"type": "byteLength", "length": "foop"})));
    }

    #[test]
    fn continues_looking_for_bytes_in_additional_writes() {
        let mut p = build(json!({"type": "byteLength", "length": 4}));
        assert_eq!(write(&mut *p, &[b"ab", b"cd"]), frames(&[b"abcd"]));
    }

    #[test]
    fn flushes_remaining_data_when_the_stream_ends() {
        let mut p = build(json!({"type": "byteLength", "length": 4}));
        assert!(write(&mut *p, &[b"12"]).is_empty());
        assert_eq!(p.flush(), Some(b"12".to_vec()));
    }
}

mod cctalk {
    use super::*;

    #[test]
    fn constructs() {
        build(json!({"type": "ccTalk"}));
    }

    #[test]
    fn emits_data_for_a_default_length_message() {
        let mut p = build(json!({"type": "ccTalk"}));
        let out = write(&mut *p, &[&[2, 0, 1, 254, 217]]);
        assert_eq!(out, frames(&[&[2, 0, 1, 254, 217]]));
    }

    #[test]
    fn emits_data_for_a_7_byte_length_message() {
        let mut p = build(json!({"type": "ccTalk"}));
        let out = write(&mut *p, &[&[2, 2, 1, 254, 1, 1, 217]]);
        assert_eq!(out, frames(&[&[2, 2, 1, 254, 1, 1, 217]]));
    }

    #[test]
    fn parses_multiple_length_messages() {
        let mut p = build(json!({"type": "ccTalk"}));
        let out = write(
            &mut *p,
            &[&[2, 2, 1], &[254, 1, 1], &[217, 2], &[0, 1, 254, 217]],
        );
        assert_eq!(
            out,
            frames(&[&[2, 2, 1, 254, 1, 1, 217], &[2, 0, 1, 254, 217]])
        );
    }

    #[test]
    fn parses_a_long_message() {
        let mut p = build(json!({"type": "ccTalk"}));
        let out = write(
            &mut *p,
            &[&[
                2, 2, 1, 254, 1, 1, 217, 2, 0, 1, 254, 217, 2, 2, 1, 251, 1, 1, 217, 2, 2, 1, 252,
                1, 1, 217, 2, 0, 1, 253, 217,
            ]],
        );
        assert_eq!(
            out,
            frames(&[
                &[2, 2, 1, 254, 1, 1, 217],
                &[2, 0, 1, 254, 217],
                &[2, 2, 1, 251, 1, 1, 217],
                &[2, 2, 1, 252, 1, 1, 217],
                &[2, 0, 1, 253, 217],
            ])
        );
    }

    #[test]
    fn resets_incomplete_message_after_timeout() {
        let mut p = build(json!({"type": "ccTalk", "maxDelayMs": 50}));
        let t0 = now();
        let mut out = Vec::new();
        p.push(&[2, 2, 1], t0, &mut out);
        p.push(
            &[2, 2, 1, 254, 1, 1, 217],
            t0 + Duration::from_millis(51),
            &mut out,
        );
        assert_eq!(out, frames(&[&[2, 2, 1, 254, 1, 1, 217]]));
    }

    #[test]
    fn disabled_message_timeout() {
        let mut p = build(json!({"type": "ccTalk", "maxDelayMs": 0}));
        let t0 = now();
        let mut out = Vec::new();
        p.push(&[2, 2, 1], t0, &mut out);
        p.push(&[254, 1, 1, 217], t0 + Duration::from_millis(100), &mut out);
        assert_eq!(out, frames(&[&[2, 2, 1, 254, 1, 1, 217]]));
    }

    #[test]
    fn node_option_name_is_accepted() {
        let mut p = build(json!({"type": "ccTalk", "maxDelayBetweenBytesMs": 0}));
        let t0 = now();
        let mut out = Vec::new();
        p.push(&[2, 0], t0, &mut out);
        p.push(&[1, 254, 217], t0 + Duration::from_secs(1), &mut out);
        assert_eq!(out, frames(&[&[2, 0, 1, 254, 217]]));
    }

    #[test]
    fn partial_frame_is_dropped_at_end_like_node() {
        let mut p = build(json!({"type": "ccTalk"}));
        assert!(write_end(&mut *p, &[&[2, 2, 1]]).is_empty());
    }
}

mod delimiter {
    use super::*;

    const LINES: [&[u8]; 4] = [
        b"I love robots\nEach ",
        b"and Every One\n",
        b"\n",
        b"even you!",
    ];

    #[test]
    fn transforms_data_to_strings_split_on_a_delimiter() {
        let mut p = build(json!({"type": "delimiter", "delimiter": "\n"}));
        assert_eq!(
            write(&mut *p, &LINES),
            frames(&[b"I love robots", b"Each and Every One"])
        );
    }

    #[test]
    fn includes_delimiter_when_include_delimiter_is_true() {
        let mut p =
            build(json!({"type": "delimiter", "delimiter": "\n", "includeDelimiter": true}));
        assert_eq!(
            write(&mut *p, &LINES),
            frames(&[b"I love robots\n", b"Each and Every One\n", b"\n"])
        );
    }

    #[test]
    fn flushes_remaining_data_when_the_stream_ends() {
        let mut p = build(json!({"type": "delimiter", "delimiter": [0]}));
        assert!(write(&mut *p, &[&[1]]).is_empty());
        assert_eq!(p.flush(), Some(vec![1]));
    }

    #[test]
    fn throws_when_not_provided_with_a_delimiter() {
        assert!(rejects(json!({"type": "delimiter"})));
        assert!(rejects(Value::Null));
    }

    #[test]
    fn throws_when_called_with_a_0_length_delimiter() {
        assert!(rejects(json!({"type": "delimiter", "delimiter": []})));
        assert!(rejects(json!({"type": "delimiter", "delimiter": ""})));
    }

    #[test]
    fn allows_setting_of_the_delimiter_with_a_string() {
        build(json!({"type": "delimiter", "delimiter": "string"}));
    }

    #[test]
    fn allows_setting_of_the_delimiter_with_a_buffer() {
        // A Node Buffer is `ByteSeq::Bytes` from Rust (a byte array in JSON).
        FramerConfig::Delimiter {
            delimiter: ByteSeq::from(&[1u8][..]),
            include_delimiter: false,
            max_buffer_size: None,
        }
        .build()
        .unwrap();
    }

    #[test]
    fn allows_setting_of_the_delimiter_with_an_array_of_bytes() {
        build(json!({"type": "delimiter", "delimiter": [1]}));
    }

    #[test]
    fn emits_data_events_every_time_it_meets_00x_00x() {
        let mut p = build(json!({"type": "delimiter", "delimiter": [0, 0]}));
        let out = write(
            &mut *p,
            &[b"This could be\0\0binary data\0\0sent from a Moteino\0\0"],
        );
        assert_eq!(
            out,
            frames(&[b"This could be", b"binary data", b"sent from a Moteino"])
        );
    }

    #[test]
    fn accepts_single_byte_delimiter() {
        let mut p = build(json!({"type": "delimiter", "delimiter": [0]}));
        let out = write(
            &mut *p,
            &[b"This could be\0binary data\0sent from a Moteino\0"],
        );
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn works_when_buffer_starts_with_delimiter() {
        let mut p = build(json!({"type": "delimiter", "delimiter": [0]}));
        let out = write(&mut *p, &[b"\0Hello\0World\0"]);
        assert_eq!(out, frames(&[b"Hello", b"World"]));
    }

    #[test]
    fn should_only_emit_if_delimiters_are_strictly_in_row() {
        let mut p = build(json!({"type": "delimiter", "delimiter": [0, 1]}));
        let out = write(&mut *p, &[b"\0Hello\x01World\0\0\x01"]);
        assert_eq!(out, frames(&[b"\0Hello\x01World\0"]));
    }

    #[test]
    fn continues_looking_for_delimiters_in_the_next_buffers() {
        let mut p = build(json!({"type": "delimiter", "delimiter": [0, 0]}));
        let out = write(
            &mut *p,
            &[
                b"This could be\0\0binary ",
                b"data\0\0sent from a Moteino\0\0",
            ],
        );
        assert_eq!(
            out,
            frames(&[b"This could be", b"binary data", b"sent from a Moteino"])
        );
    }

    #[test]
    fn works_if_a_multibyte_delimiter_crosses_a_chunk_boundary() {
        let mut p = build(json!({"type": "delimiter", "delimiter": [0, 1]}));
        let out = write(&mut *p, &[&[1, 2, 3, 0], &[1, 2, 3, 0], &[1]]);
        assert_eq!(out, frames(&[&[1, 2, 3], &[2, 3]]));
    }

    #[test]
    fn bounded_buffer_emits_an_overlong_line() {
        let mut p = build(json!({"type": "delimiter", "delimiter": "\n", "maxBufferSize": 4}));
        assert_eq!(write(&mut *p, &[b"abcdef"]), frames(&[b"abcd"]));
        assert_eq!(write(&mut *p, &[b"\n"]), frames(&[b"ef"]));
        assert!(rejects(
            json!({"type": "delimiter", "delimiter": "\n", "maxBufferSize": 0})
        ));
    }
}

mod readline {
    use super::*;

    #[test]
    fn transforms_data_to_strings_split_on_a_delimiter() {
        let mut p = build(json!({"type": "readline"}));
        let out = write(
            &mut *p,
            &[b"I love robots\nEach ", b"and Every One\n", b"even you!"],
        );
        assert_eq!(out, frames(&[b"I love robots", b"Each and Every One"]));
        assert_eq!(p.flush(), Some(b"even you!".to_vec()));
    }

    #[test]
    fn allows_setting_of_the_delimiter_with_a_string() {
        let mut p = build(json!({"type": "readline", "delimiter": "a"}));
        assert_eq!(
            write(&mut *p, &[b"how are youa"]),
            frames(&[b"how ", b"re you"])
        );
    }

    #[test]
    fn allows_setting_of_the_delimiter_with_a_buffer() {
        let mut p = FramerConfig::Readline {
            delimiter: Some(ByteSeq::from(&b"a"[..])),
            include_delimiter: false,
            max_buffer_size: None,
            encoding: None,
        }
        .build()
        .unwrap();
        assert_eq!(
            write(&mut *p, &[b"how are youa"]),
            frames(&[b"how ", b"re you"])
        );
    }

    #[test]
    fn allows_setting_of_the_delimiter_with_an_array_of_bytes() {
        let mut p = build(json!({"type": "readline", "delimiter": [97]}));
        assert_eq!(
            write(&mut *p, &[b"how are youa"]),
            frames(&[b"how ", b"re you"])
        );
    }

    /// Node renders frames as `'61'`, `'62'` with `encoding: 'hex'`. Not applicable:
    /// frames are bytes, the option is ignored, the bytes are the same.
    #[test]
    fn allows_setting_of_encoding() {
        let mut p = build(json!({"type": "readline", "encoding": "hex"}));
        assert_eq!(write(&mut *p, &[b"a\nb\n"]), frames(&[b"a", b"b"]));
    }

    /// Node decodes a string delimiter with `encoding` (`'FF'` as hex = `[0xff]`).
    /// Not applicable: a string delimiter is UTF-8; give bytes instead.
    #[test]
    fn encoding_should_be_reflected_in_a_string_delimiter() {
        let mut p = build(json!({"type": "readline", "delimiter": [255], "encoding": "hex"}));
        assert_eq!(write(&mut *p, &[&[0, 255, 1, 255]]), frames(&[&[0], &[1]]));
    }

    #[test]
    fn throws_when_called_with_a_0_length_delimiter() {
        assert!(rejects(json!({"type": "readline", "delimiter": []})));
        assert!(rejects(json!({"type": "readline", "delimiter": ""})));
    }

    #[test]
    fn doesnt_emit_empty_data_events() {
        let mut p = build(json!({"type": "readline", "delimiter": "a"}));
        assert_eq!(write(&mut *p, &[b"aFa"]), frames(&[b"F"]));
    }
}

mod ready {
    use super::*;

    #[test]
    fn emits_data_received_after_the_ready_data() {
        let mut p = build(json!({"type": "ready", "delimiter": "\n"}));
        let out = write(
            &mut *p,
            &[b"which will you get?", b"garbage\ngold", b"just for you"],
        );
        assert_eq!(out, frames(&[b"gold", b"just for you"]));
    }

    #[test]
    fn emits_the_ready_event_before_the_data_event() {
        let mut p = build(json!({"type": "ready", "delimiter": "!"}));
        assert_eq!(write(&mut *p, &[b"!hi"]), frames(&[b"hi"]));
    }

    #[test]
    fn has_a_ready_property() {
        let mut p = ReadyFramer {
            delimiter: b"\n".to_vec(),
            tail: Vec::new(),
            ready: false,
        };
        assert!(!p.ready);
        write(&mut p, &[b"not the new line"]);
        assert!(!p.ready);
        write(&mut p, &[b"this is the \n"]);
        assert!(p.ready);
    }

    #[test]
    fn throws_when_not_provided_with_a_delimiter() {
        assert!(rejects(Value::Null));
        assert!(rejects(json!({"type": "ready"})));
    }

    #[test]
    fn throws_when_called_with_a_0_length_delimiter() {
        assert!(rejects(json!({"type": "ready", "delimiter": []})));
        assert!(rejects(json!({"type": "ready", "delimiter": ""})));
    }

    #[test]
    fn allows_setting_of_the_delimiter_with_a_string() {
        build(json!({"type": "ready", "delimiter": "string"}));
    }

    #[test]
    fn allows_setting_of_the_delimiter_with_a_buffer() {
        FramerConfig::Ready {
            delimiter: ByteSeq::from(&[1u8][..]),
        }
        .build()
        .unwrap();
    }

    #[test]
    fn allows_setting_of_the_delimiter_with_an_array_of_bytes() {
        build(json!({"type": "ready", "delimiter": [1]}));
    }

    #[test]
    fn allows_receiving_the_delimiter_over_small_writes() {
        let mut p = build(json!({"type": "ready", "delimiter": "READY"}));
        let out = write(&mut *p, &[b"bad data then REA", b"D", b"Y", b"!!!!!!"]);
        assert_eq!(out, frames(&[b"!!!!!!"]));
    }

    /// Node misses this: after `R`,`R` it resets without re-checking the second `R`.
    #[test]
    fn finds_the_marker_after_a_false_start() {
        let mut p = build(json!({"type": "ready", "delimiter": "READY"}));
        assert_eq!(write(&mut *p, &[b"RREADY!"]), frames(&[b"!"]));
        let mut p = build(json!({"type": "ready", "delimiter": "READY"}));
        assert_eq!(write(&mut *p, &[b"xRR", b"EA", b"DY!"]), frames(&[b"!"]));
        let mut p = build(json!({"type": "ready", "delimiter": "aab"}));
        assert_eq!(write(&mut *p, &[b"aaab", b"z"]), frames(&[b"z"]));
    }

    #[test]
    fn nothing_before_ready_is_flushed() {
        let mut p = build(json!({"type": "ready", "delimiter": "READY"}));
        assert!(write_end(&mut *p, &[b"REA"]).is_empty());
    }
}

mod regex {
    use super::*;

    #[test]
    fn transforms_data_to_strings_split_on_either_carriage_return_or_new_line() {
        let mut p = build(json!({"type": "regex", "regex": "[\\r\\n]+"}));
        let out = write(
            &mut *p,
            &[
                b"I love robots\r\nEach ",
                b"and Every One\r",
                b"even you!",
                b"\nThe angry red robot",
            ],
        );
        assert_eq!(
            out,
            frames(&[b"I love robots", b"Each and Every One", b"even you!"])
        );
    }

    #[test]
    fn flushes_remaining_data_when_the_stream_ends() {
        let mut p = build(json!({"type": "regex", "regex": "\\n"}));
        assert!(write(&mut *p, &[&[1]]).is_empty());
        assert_eq!(p.flush(), Some(vec![1]));
    }

    #[test]
    fn throws_when_not_provided_with_a_regex() {
        assert!(rejects(json!({"type": "regex"})));
    }

    #[test]
    fn throws_when_called_with_an_invalid_regex_expression() {
        assert!(rejects(json!({"type": "regex", "regex": "\\"})));
    }

    #[test]
    fn allows_setting_of_the_regex_with_a_regex_string() {
        let mut p = build(json!({"type": "regex", "regex": "a|b"}));
        assert_eq!(
            write(&mut *p, &[b"bhow are youa"]),
            frames(&[b"how ", b"re you"])
        );
    }

    /// Node turns a Buffer pattern into its string; here the pattern is always a `String`.
    #[test]
    fn allows_setting_of_the_regex_with_a_buffer() {
        let regex = String::from_utf8(b"a|b".to_vec()).unwrap();
        let mut p = FramerConfig::Regex {
            regex,
            max_buffer_size: None,
            encoding: None,
        }
        .build()
        .unwrap();
        assert_eq!(
            write(&mut *p, &[b"bhow are youa"]),
            frames(&[b"how ", b"re you"])
        );
    }

    /// Node renders `'61'`, `'62'` with `encoding: 'hex'`; not applicable, bytes are the same.
    #[test]
    fn allows_setting_of_encoding() {
        let mut p = build(json!({"type": "regex", "regex": "\\r", "encoding": "hex"}));
        assert_eq!(write(&mut *p, &[b"a\rb\r"]), frames(&[b"a", b"b"]));
    }

    #[test]
    fn works_when_buffer_starts_with_regex() {
        let mut p = build(json!({"type": "regex", "regex": "\\r"}));
        assert_eq!(
            write(&mut *p, &[b"\rHello\rWorld\r"]),
            frames(&[b"Hello", b"World"])
        );
    }

    #[test]
    fn should_match_unicode_in_buffer_string() {
        let mut p = build(json!({"type": "regex", "regex": "\\r\\n|\\n"}));
        let out = write(
            &mut *p,
            &["\u{000a}Hello\u{000a}World\u{000d}\u{000a}!".as_bytes()],
        );
        assert_eq!(out, frames(&[b"Hello", b"World"]));
    }

    #[test]
    fn continues_looking_for_regexs_in_the_next_buffers() {
        let mut p = build(json!({"type": "regex", "regex": "\\r\\n|\\n"}));
        let out = write(
            &mut *p,
            &[
                b"This could be\na poem ",
                b"or prose\r\nsent from a robot\r\n",
            ],
        );
        assert_eq!(
            out,
            frames(&[b"This could be", b"a poem or prose", b"sent from a robot"])
        );
    }

    #[test]
    fn doesnt_emit_empty_data_events() {
        let mut p = build(json!({"type": "regex", "regex": "a|b"}));
        assert_eq!(write(&mut *p, &[b"abaFab"]), frames(&[b"F"]));
    }

    /// Node decodes each chunk alone and would emit `"x\u{FFFD}\u{FFFD}"`.
    #[test]
    fn utf8_character_split_across_chunks_is_kept() {
        let mut p = build(json!({"type": "regex", "regex": "\\n"}));
        let out = write(&mut *p, &[&[b'x', 0xc3], &[0xa9, b'\n']]);
        assert_eq!(out, frames(&["xé".as_bytes()]));
    }

    #[test]
    fn bounded_buffer_emits_overlong_text() {
        let mut p = build(json!({"type": "regex", "regex": "\\n", "maxBufferSize": 4}));
        assert_eq!(
            write(&mut *p, &[b"ab", b"cd", b"e\n"]),
            frames(&[b"abcd", b"e"])
        );
    }
}

mod slip_decoder {
    use super::*;

    fn custom() -> Value {
        json!({
            "type": "slip",
            "START": 0xab, "ESC": 0xcd, "END": 0xbc,
            "ESC_START": 0xac, "ESC_ESC": 0xce, "ESC_END": 0xbd,
        })
    }

    #[test]
    fn decodes_one_byte_messages() {
        let mut p = build(json!({"type": "slip"}));
        let out = write(
            &mut *p,
            &[&[0x01, 0xc0], &[0x80, 0xc0], &[0xff, 0xc0], &[0xa5, 0xc0]],
        );
        assert_eq!(out, frames(&[&[0x01], &[0x80], &[0xff], &[0xa5]]));
    }

    #[test]
    fn no_data_event_on_zero_byte_messages() {
        let mut p = build(json!({"type": "slip"}));
        assert!(write(&mut *p, &[&[0xc0]]).is_empty());
    }

    #[test]
    fn decodes_escaped_characters() {
        let mut p = build(json!({"type": "slip"}));
        let out = write(
            &mut *p,
            &[
                &[0x01, 0xc0],
                &[0xdb, 0xdc, 0xc0],
                &[0xdb, 0xdd, 0xc0],
                &[0xdc, 0xc0],
                &[0xdd, 0xc0],
                &[0xff, 0xc0],
            ],
        );
        assert_eq!(
            out,
            frames(&[&[0x01], &[0xc0], &[0xdb], &[0xdc], &[0xdd], &[0xff]])
        );
    }

    #[test]
    fn decodes_escaped_characters_with_custom_escapes() {
        let mut p = build(custom());
        let out = write(
            &mut *p,
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
        assert_eq!(
            out,
            frames(&[
                &[0x01],
                &[0xbc],
                &[0xcd],
                &[0xdc],
                &[0xdd],
                &[0xff],
                &[0xab]
            ])
        );
    }

    #[test]
    fn decodes_invalid_escape() {
        let mut p = build(custom());
        let out = write(&mut *p, &[&[0xab, 0x01, 0xcd, 0x02, 0xbc]]);
        assert_eq!(out, frames(&[&[0x01], &[0x02]]));
    }

    #[test]
    fn decodes_data_split_across_multiple_chunks() {
        let mut p = build(custom());
        let out = write(&mut *p, &[&[0xab, 0x01], &[0xcd, 0xbd, 0xbc]]);
        assert_eq!(out, frames(&[&[0x01, 0xbc]]));
    }

    #[test]
    fn decodes_data_split_across_multiple_chunks_on_escape() {
        let mut p = build(custom());
        let out = write(&mut *p, &[&[0xab, 0x01, 0xcd], &[0xbd, 0xbc]]);
        assert_eq!(out, frames(&[&[0x01, 0xbc]]));
    }

    #[test]
    fn data_before_start_is_dropped() {
        let mut p = build(json!({"type": "slip", "START": 0xab, "END": 0xbc}));
        let out = write(&mut *p, &[&[0x01, 0x02, 0xdb], &[0xab, 0x03, 0x04, 0xbc]]);
        assert_eq!(out, frames(&[&[0x03, 0x04]]));
    }

    #[test]
    fn flushes_pending_data_on_stream_end() {
        let mut p = build(custom());
        assert!(write(&mut *p, &[&[0xab, 0x01, 0xcd]]).is_empty());
        assert_eq!(p.flush(), Some(vec![0x01]));
    }

    #[test]
    fn camel_case_option_names_are_accepted() {
        let mut p = build(json!({"type": "slip", "start": 0xab, "end": 0xbc}));
        assert_eq!(
            write(&mut *p, &[&[0x01, 0xab, 0x02, 0xbc]]),
            frames(&[&[0x02]])
        );
    }

    #[test]
    fn bounded_buffer_emits_an_endless_packet() {
        let mut p = build(json!({"type": "slip"}));
        let out = write(&mut *p, &[&vec![1u8; DEFAULT_MAX_BUFFER + 1]]);
        assert_eq!(out, vec![vec![1u8; DEFAULT_MAX_BUFFER]]);
        assert_eq!(p.flush(), Some(vec![1]));
    }
}

mod slip_encoder {
    use super::*;

    fn enc(o: &SlipOptions, quirk: bool, packets: &[&[u8]]) -> Vec<Vec<u8>> {
        // Node emits nothing for an empty bluetooth-quirk packet.
        packets
            .iter()
            .map(|p| slip_encode(p, o, quirk))
            .filter(|e| !e.is_empty())
            .collect()
    }

    fn custom() -> SlipOptions {
        SlipOptions {
            start: Some(0xab),
            esc: 0xcd,
            end: 0xbc,
            esc_start: Some(0xac),
            esc_esc: 0xce,
            esc_end: 0xbd,
        }
    }

    const ONE_BYTE: [&[u8]; 4] = [&[0x01], &[0x80], &[0xff], &[0xa5]];
    const ESCAPES: [&[u8]; 6] = [&[0x01], &[0xc0], &[0xdb], &[0xdc], &[0xdd], &[0xff]];

    #[test]
    fn adds_one_delimiter_to_one_byte_messages() {
        let out = enc(&SlipOptions::default(), false, &ONE_BYTE);
        assert_eq!(
            out,
            frames(&[&[0x01, 0xc0], &[0x80, 0xc0], &[0xff, 0xc0], &[0xa5, 0xc0]])
        );
    }

    #[test]
    fn adds_two_delimiters_to_one_byte_messages_with_the_bluetooth_quirk() {
        let out = enc(&SlipOptions::default(), true, &ONE_BYTE);
        assert_eq!(
            out,
            frames(&[
                &[0xc0, 0x01, 0xc0],
                &[0xc0, 0x80, 0xc0],
                &[0xc0, 0xff, 0xc0],
                &[0xc0, 0xa5, 0xc0],
            ])
        );
    }

    #[test]
    fn adds_one_delimiter_to_zero_byte_messages() {
        assert_eq!(
            enc(&SlipOptions::default(), false, &[&[]]),
            frames(&[&[0xc0]])
        );
    }

    #[test]
    fn does_nothing_with_zero_byte_messages_with_the_bluetooth_quirk() {
        assert!(enc(&SlipOptions::default(), true, &[&[], &[], &[], &[]]).is_empty());
    }

    #[test]
    fn escapes_characters() {
        let out = enc(&SlipOptions::default(), false, &ESCAPES);
        assert_eq!(
            out,
            frames(&[
                &[0x01, 0xc0],
                &[0xdb, 0xdc, 0xc0],
                &[0xdb, 0xdd, 0xc0],
                &[0xdc, 0xc0],
                &[0xdd, 0xc0],
                &[0xff, 0xc0],
            ])
        );
    }

    #[test]
    fn escapes_characters_with_custom_escapes() {
        let out = enc(
            &custom(),
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
        );
        assert_eq!(
            out,
            frames(&[
                &[0xab, 0x01, 0xbc],
                &[0xab, 0xcd, 0xbd, 0xbc],
                &[0xab, 0xcd, 0xce, 0xbc],
                &[0xab, 0xdc, 0xbc],
                &[0xab, 0xdd, 0xbc],
                &[0xab, 0xff, 0xbc],
                &[0xab, 0xcd, 0xac, 0xbc],
            ])
        );
    }

    #[test]
    fn escapes_characters_with_the_bluetooth_quirk() {
        let out = enc(&SlipOptions::default(), true, &ESCAPES);
        assert_eq!(
            out,
            frames(&[
                &[0xc0, 0x01, 0xc0],
                &[0xc0, 0xdb, 0xdc, 0xc0],
                &[0xc0, 0xdb, 0xdd, 0xc0],
                &[0xc0, 0xdc, 0xc0],
                &[0xc0, 0xdd, 0xc0],
                &[0xc0, 0xff, 0xc0],
            ])
        );
    }

    #[test]
    fn encoder_and_decoder_round_trip() {
        for o in [SlipOptions::default(), custom()] {
            let data: Vec<u8> = (0..=255).collect();
            let mut d = SlipDecoder::new(o.clone());
            let out = write(&mut d, &[&slip_encode(&data, &o, true)]);
            assert_eq!(out, vec![data]);
        }
    }
}

mod start_end {
    use super::*;

    const INPUT: [&[u8]; 4] = [
        b"\x02I love robots\x03\x02Each ",
        b"and Every One\x03",
        b"\x02",
        b"even you!",
    ];

    fn cfg(include_start: bool, include_end: bool) -> Value {
        json!({
            "type": "startEnd", "startDelimiter": "\u{2}", "endDelimiter": "\u{3}",
            "includeStartDelimiter": include_start, "includeEndDelimiter": include_end,
        })
    }

    #[test]
    fn transforms_data_to_strings_split_on_start_delimiter_and_end_delimiter() {
        let out = write(&mut *build(cfg(false, false)), &INPUT);
        assert_eq!(out, frames(&[b"I love robots", b"Each and Every One"]));
    }

    #[test]
    fn includes_start_delimiter_when_include_start_delimiter_is_true() {
        let out = write(&mut *build(cfg(true, false)), &INPUT);
        assert_eq!(
            out,
            frames(&[b"\x02I love robots", b"\x02Each and Every One"])
        );
    }

    #[test]
    fn includes_end_delimiter_when_include_end_delimiter_is_true() {
        let out = write(&mut *build(cfg(false, true)), &INPUT);
        assert_eq!(
            out,
            frames(&[b"I love robots\x03", b"Each and Every One\x03"])
        );
    }

    #[test]
    fn includes_both_delimiters_when_both_are_true() {
        let out = write(&mut *build(cfg(true, true)), &INPUT);
        assert_eq!(
            out,
            frames(&[b"\x02I love robots\x03", b"\x02Each and Every One\x03"])
        );
    }

    #[test]
    fn flushes_remaining_data_when_the_stream_ends() {
        let mut p = build(cfg(false, false));
        assert!(write(&mut *p, &[&[1]]).is_empty());
        assert_eq!(p.flush(), Some(vec![1]));
    }

    #[test]
    fn throws_when_not_provided_with_a_start_delimiter() {
        assert!(rejects(
            json!({"type": "startEnd", "endDelimiter": "\u{3}"})
        ));
    }

    #[test]
    fn throws_when_not_provided_with_an_end_delimiter() {
        assert!(rejects(
            json!({"type": "startEnd", "startDelimiter": "\u{2}"})
        ));
    }

    #[test]
    fn throws_when_called_with_a_0_length_start_delimiter() {
        for d in [json!([]), json!("")] {
            assert!(rejects(
                json!({"type": "startEnd", "startDelimiter": d, "endDelimiter": "\u{3}"})
            ));
        }
    }

    #[test]
    fn throws_when_called_with_a_0_length_end_delimiter() {
        for d in [json!([]), json!("")] {
            assert!(rejects(
                json!({"type": "startEnd", "startDelimiter": "\u{2}", "endDelimiter": d})
            ));
        }
    }

    #[test]
    fn allows_setting_of_the_delimiters_with_strings() {
        build(json!({"type": "startEnd", "startDelimiter": "string", "endDelimiter": "string"}));
    }

    #[test]
    fn allows_setting_of_the_delimiters_with_buffers() {
        FramerConfig::StartEnd {
            start_delimiter: ByteSeq::from(&[1u8][..]),
            end_delimiter: ByteSeq::from(&[1u8][..]),
            include_start_delimiter: false,
            include_end_delimiter: false,
            max_buffer_size: None,
        }
        .build()
        .unwrap();
    }

    #[test]
    fn allows_setting_of_the_delimiters_with_arrays_of_bytes() {
        build(json!({"type": "startEnd", "startDelimiter": [1], "endDelimiter": [1]}));
    }

    #[test]
    fn works_when_buffer_starts_with_start_delimiter_end_delimiter() {
        let out = write(
            &mut *build(cfg(false, false)),
            &[b"\x02\x03\x02Hello\x03\x02World\x03"],
        );
        assert_eq!(out, frames(&[b"Hello", b"World"]));
    }

    #[test]
    fn continues_looking_for_delimiters_in_the_next_buffers() {
        let out = write(
            &mut *build(cfg(false, false)),
            &[
                b"\x02This could be\x03\x02binary ",
                b"data\x03\x02sent from a Moteino\x03",
            ],
        );
        assert_eq!(
            out,
            frames(&[b"This could be", b"binary data", b"sent from a Moteino"])
        );
    }

    #[test]
    fn works_if_a_multibyte_delimiter_crosses_a_chunk_boundary() {
        let mut p =
            build(json!({"type": "startEnd", "startDelimiter": [7, 7], "endDelimiter": [8, 8]}));
        let out = write(&mut *p, &[&[1, 2, 3, 7], &[7, 2, 3, 8], &[8]]);
        assert_eq!(out, frames(&[&[2, 3]]));
    }

    #[test]
    fn bounded_buffer_drops_an_overlong_open_frame() {
        let mut p = build(json!({
            "type": "startEnd", "startDelimiter": "\u{2}", "endDelimiter": "\u{3}",
            "maxBufferSize": 8,
        }));
        assert_eq!(
            write(&mut *p, &[b"\x02too long for it", b"\x02ok", b"\x03"]),
            frames(&[b"ok"])
        );
        let mut p = build(json!({
            "type": "startEnd", "startDelimiter": "\u{2}", "endDelimiter": "\u{3}",
            "maxBufferSize": 8,
        }));
        // The later start marker survives the cut.
        assert_eq!(
            write(&mut *p, &[b"\x02junk\x02ok", b"\x03"]),
            frames(&[b"ok"])
        );
    }
}

mod packet_length {
    use super::*;

    /// Node's `Buffer.from('\xaa…')` is UTF-8: `\xaa` becomes `c2 aa`.
    fn s(text: &str) -> &[u8] {
        text.as_bytes()
    }

    fn cat(head: &[u8], tail: &str) -> Vec<u8> {
        [head, tail.as_bytes()].concat()
    }

    fn multi() -> Value {
        json!({"type": "packetLength", "lengthOffset": 2, "packetOverhead": 4, "lengthBytes": 2})
    }

    #[test]
    fn transforms_data_to_packets_of_correct_length_starting_with_delimiter() {
        let mut p = build(json!({"type": "packetLength"}));
        let out = write(
            &mut *p,
            &[
                s("\u{aa}\u{0d}I love robots\u{aa}\u{13}Each "),
                s("and Every One\n"),
                &[0xaa, 0x24],
                s("even you!"),
            ],
        );
        assert_eq!(
            out,
            vec![
                cat(&[0xaa, 0x0d], "I love robots"),
                cat(&[0xaa, 0x13], "Each and Every One\n")
            ]
        );
    }

    #[test]
    fn when_length_is_offset() {
        let mut p = build(json!({"type": "packetLength", "lengthOffset": 2, "packetOverhead": 3}));
        let out = write(
            &mut *p,
            &[
                s("\u{aa}\u{01}\u{0d}I love robots\u{aa}\u{02}\u{13}Each "),
                s("and Every One\n"),
                &[0xaa, 0x03, 0x24],
                s("even you!"),
            ],
        );
        assert_eq!(
            out,
            vec![
                cat(&[0xaa, 0x01, 0x0d], "I love robots"),
                cat(&[0xaa, 0x02, 0x13], "Each and Every One\n")
            ]
        );
    }

    const MULTI_INPUT: [&str; 2] = [
        "\u{aa}\u{01}\u{0d}\u{00}I love robots\u{aa}\u{02}\u{13}\u{00}Each ",
        "and Every One\n",
    ];

    #[test]
    fn when_length_is_offset_and_multibyte() {
        let mut p = build(multi());
        let out = write(
            &mut *p,
            &[
                s(MULTI_INPUT[0]),
                s(MULTI_INPUT[1]),
                &[0xaa, 0x03, 0x24, 0x00],
                s("even you!"),
            ],
        );
        assert_eq!(
            out,
            vec![
                cat(&[0xaa, 0x01, 0x0d, 0x00], "I love robots"),
                cat(&[0xaa, 0x02, 0x13, 0x00], "Each and Every One\n")
            ]
        );
    }

    #[test]
    fn flushes_remaining_data_when_the_stream_ends() {
        let mut p = build(multi());
        let out = write_end(
            &mut *p,
            &[
                s(MULTI_INPUT[0]),
                s(MULTI_INPUT[1]),
                &[0xaa, 0x03, 0x24, 0x00],
                s("even you!"),
            ],
        );
        assert_eq!(
            out,
            vec![
                cat(&[0xaa, 0x01, 0x0d, 0x00], "I love robots"),
                cat(&[0xaa, 0x02, 0x13, 0x00], "Each and Every One\n"),
                cat(&[0xaa, 0x03, 0x24, 0x00], "even you!"),
            ]
        );
    }

    #[test]
    fn emits_data_early_when_invalid_length_encountered() {
        let mut p = build(json!({
            "type": "packetLength", "lengthOffset": 2, "packetOverhead": 4,
            "lengthBytes": 2, "maxLen": 0x0d,
        }));
        let out = write(
            &mut *p,
            &[
                s(MULTI_INPUT[0]),
                s(MULTI_INPUT[1]),
                &[0xaa, 0x03, 0x09, 0x00],
                s("even you!"),
            ],
        );
        assert_eq!(
            out,
            vec![
                cat(&[0xaa, 0x01, 0x0d, 0x00], "I love robots"),
                vec![0xaa, 0x02, 0x13, 0x00],
                cat(&[0xaa, 0x03, 0x09, 0x00], "even you!"),
            ]
        );
    }

    #[test]
    fn works_if_a_multibyte_length_crosses_a_chunk_boundary() {
        let mut p = build(multi());
        let out = write(
            &mut *p,
            &[
                s("\u{aa}\u{01}\u{0d}"),
                s("\u{00}I love robots\u{aa}\u{02}\u{13}\u{00}Each "),
                s("and Every One\n"),
                &[0xaa, 0x03, 0x24, 0x00],
                s("even you!"),
            ],
        );
        assert_eq!(
            out,
            vec![
                cat(&[0xaa, 0x01, 0x0d, 0x00], "I love robots"),
                cat(&[0xaa, 0x02, 0x13, 0x00], "Each and Every One\n")
            ]
        );
    }

    #[test]
    fn works_with_multiple_delimiters() {
        let mut p = build(json!({
            "type": "packetLength", "delimiter": [0xaa, 0xbb],
            "lengthOffset": 2, "packetOverhead": 4, "lengthBytes": 2,
        }));
        let out = write(
            &mut *p,
            &[
                s("\u{bb}\u{01}\u{0d}"),
                s("\u{00}I love hobits\u{aa}\u{02}\u{13}\u{00}Each "),
                s("and Every One\n"),
            ],
        );
        assert_eq!(
            out,
            vec![
                cat(&[0xbb, 0x01, 0x0d, 0x00], "I love hobits"),
                cat(&[0xaa, 0x02, 0x13, 0x00], "Each and Every One\n")
            ]
        );
    }

    #[test]
    fn works_with_multibyte_delimiters() {
        let mut p = build(json!({
            "type": "packetLength", "delimiter": [0xababba], "delimiterBytes": 3,
            "lengthOffset": 3, "packetOverhead": 4, "lengthBytes": 1,
        }));
        let out = write(
            &mut *p,
            &[
                &[0xab, 0xab, 0xba, 0x0d],
                s("I love hobits"),
                &[0xab, 0xab, 0xba, 0x13],
                s("Each and Every One\n"),
            ],
        );
        assert_eq!(
            out,
            vec![
                cat(&[0xab, 0xab, 0xba, 0x0d], "I love hobits"),
                cat(&[0xab, 0xab, 0xba, 0x13], "Each and Every One\n")
            ]
        );
    }

    #[test]
    fn works_with_multiple_multibyte_delimiters() {
        let mut p = build(json!({
            "type": "packetLength", "delimiter": [0xababba, 0xbaddad], "delimiterBytes": 3,
            "lengthOffset": 3, "packetOverhead": 4, "lengthBytes": 1,
        }));
        let out = write(
            &mut *p,
            &[
                &[0xab, 0xab, 0xba, 0x0d],
                s("I love hobits"),
                &[0xba, 0xdd, 0xad, 0x13],
                s("Each and Every One\n"),
            ],
        );
        assert_eq!(
            out,
            vec![
                cat(&[0xab, 0xab, 0xba, 0x0d], "I love hobits"),
                cat(&[0xba, 0xdd, 0xad, 0x13], "Each and Every One\n")
            ]
        );
    }

    /// Node drops the newest byte on a mismatch and never finds these.
    #[test]
    fn finds_a_multibyte_delimiter_after_garbage() {
        let cfg = json!({
            "type": "packetLength", "delimiter": 0xababba, "delimiterBytes": 3,
            "lengthOffset": 3, "packetOverhead": 4, "lengthBytes": 1,
        });
        for garbage in [&[0x00][..], &[0xab], &[0x01, 0x02, 0xab, 0xab]] {
            let mut p = build(cfg.clone());
            let out = write(&mut *p, &[garbage, &[0xab, 0xab, 0xba, 0x02, b'h', b'i']]);
            assert_eq!(
                out,
                vec![vec![0xab, 0xab, 0xba, 0x02, b'h', b'i']],
                "{garbage:?}"
            );
        }
    }

    /// Node would flush the partial delimiter window (byte-reversed).
    #[test]
    fn flush_drops_bytes_outside_a_packet() {
        let mut p = build(
            json!({"type": "packetLength", "delimiter": [0xababba], "delimiterBytes": 3, "lengthOffset": 3}),
        );
        assert!(write_end(&mut *p, &[&[0x01, 0xab, 0xab]]).is_empty());
    }

    #[test]
    fn packet_ends_even_if_the_header_outgrows_len_plus_overhead() {
        let mut p = build(json!({"type": "packetLength", "packetOverhead": 0}));
        let out = write(&mut *p, &[&[0xaa, 0x00, 0x01, 0xaa, 0x01, 0x07]]);
        assert_eq!(out, frames(&[&[0xaa, 0x00], &[0xaa, 0x01]]));
    }

    #[test]
    fn rejects_unusable_options() {
        assert!(rejects(json!({"type": "packetLength", "delimiter": []})));
        assert!(rejects(
            json!({"type": "packetLength", "delimiterBytes": 0})
        ));
        assert!(rejects(
            json!({"type": "packetLength", "delimiterBytes": 5})
        ));
        assert!(rejects(json!({"type": "packetLength", "lengthBytes": 0})));
        assert!(rejects(json!({"type": "packetLength", "lengthBytes": 9})));
        assert!(rejects(
            json!({"type": "packetLength", "delimiterBytes": 2, "lengthOffset": 1})
        ));
    }
}

mod inter_byte_timeout {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn emits_data_events_after_a_pause_of_30ms() {
        let mut p = build(json!({"type": "interByteTimeout", "interval": 30}));
        let t0 = now();
        let mut out = Vec::new();
        p.push(b"I love robots Each", t0, &mut out);
        p.push(b"and Every One", t0, &mut out);
        assert_eq!(p.deadline(), Some(t0 + ms(30)));
        p.poll(t0 + ms(29), &mut out);
        assert!(out.is_empty());
        p.poll(t0 + ms(30), &mut out);
        assert_eq!(p.deadline(), None);
        p.push(b"even you!", t0 + ms(30), &mut out);
        p.push(b"The angry red robot", t0 + ms(30), &mut out);
        p.poll(t0 + ms(60), &mut out);
        assert_eq!(
            out,
            frames(&[
                b"I love robots Eachand Every One",
                b"even you!The angry red robot"
            ])
        );
    }

    #[test]
    fn throws_when_interval_is_not_a_number_or_negative() {
        assert!(rejects(
            json!({"type": "interByteTimeout", "interval": -20})
        ));
        assert!(rejects(
            json!({"type": "interByteTimeout", "interval": f64::NAN})
        ));
        assert!(rejects(
            json!({"type": "interByteTimeout", "interval": "hello"})
        ));
        assert!(rejects(json!({"type": "interByteTimeout"})));
        assert!(rejects(json!({"type": "interByteTimeout", "interval": 0})));
    }

    #[test]
    fn throws_when_max_buffer_size_is_not_a_number_or_negative() {
        // NaN has no JSON form (serde_json writes null, which means "unset" here).
        for m in [json!(-20), json!("hello"), json!(0)] {
            assert!(rejects(
                json!({"type": "interByteTimeout", "interval": 15, "maxBufferSize": m})
            ));
        }
    }

    #[test]
    fn emits_data_events_when_buffer_is_full() {
        let mut p = build(json!({"type": "interByteTimeout", "interval": 15, "maxBufferSize": 2}));
        let t0 = now();
        let mut out = Vec::new();
        p.push(&[1, 2, 3, 4, 5, 6], t0, &mut out);
        p.poll(t0 + ms(15), &mut out);
        assert_eq!(out, frames(&[&[1, 2], &[3, 4], &[5, 6]]));
    }

    #[test]
    fn emits_all_buffered_data_when_stream_ends() {
        let mut p = build(json!({"type": "interByteTimeout", "interval": 15}));
        assert_eq!(write_end(&mut *p, &[b"Oh wow."]), frames(&[b"Oh wow."]));
    }

    #[test]
    fn handles_not_having_any_buffered_data_when_stream_ends() {
        let mut p = build(json!({"type": "interByteTimeout", "interval": 15}));
        assert!(write_end(&mut *p, &[b""]).is_empty());
    }

    #[test]
    fn late_push_does_not_merge_with_a_timed_out_packet() {
        let mut p = build(json!({"type": "interByteTimeout", "intervalMs": 10}));
        let t0 = now();
        let mut out = Vec::new();
        p.push(b"a", t0, &mut out);
        p.push(b"b", t0 + ms(50), &mut out);
        assert_eq!(out, frames(&[b"a"]));
        assert_eq!(p.flush(), Some(b"b".to_vec()));
    }
}

/// Chunking must not change framing: feed every input one byte at a time.
mod chunk_boundaries {
    use super::*;

    #[test]
    fn delimiter() {
        same_bytewise(
            json!({"type": "delimiter", "delimiter": [0, 1], "includeDelimiter": true}),
            &[&[1, 2, 3, 0], &[1, 2, 3, 0], &[1], b"\0\0\x01tail"],
        );
        same_bytewise(
            json!({"type": "delimiter", "delimiter": "ab"}),
            &[b"xaab", b"aaabyab", b"b"],
        );
    }

    #[test]
    fn readline() {
        same_bytewise(
            json!({"type": "readline", "delimiter": "\r\n"}),
            &[b"OK\r", b"\nRING\r\n\r\n+CMT", b"I: 1\r\nrest"],
        );
    }

    #[test]
    fn byte_length() {
        same_bytewise(
            json!({"type": "byteLength", "length": 8}),
            &[b"Robots are so freaking cool!"],
        );
    }

    #[test]
    fn inter_byte_timeout() {
        same_bytewise(
            json!({"type": "interByteTimeout", "interval": 15, "maxBufferSize": 4}),
            &[b"abcdefghij"],
        );
    }

    #[test]
    fn packet_length() {
        same_bytewise(
            json!({"type": "packetLength", "lengthOffset": 2, "packetOverhead": 4, "lengthBytes": 2}),
            &[
                "\u{aa}\u{01}\u{0d}\u{00}I love robots\u{aa}\u{02}\u{13}\u{00}Each ".as_bytes(),
                b"and Every One\n",
                &[0xaa, 0x03, 0x24, 0x00],
                b"even you!",
            ],
        );
        same_bytewise(
            json!({
                "type": "packetLength", "delimiter": [0xababba, 0xbaddad], "delimiterBytes": 3,
                "lengthOffset": 3, "packetOverhead": 4, "lengthBytes": 1,
            }),
            &[
                &[0x00, 0xab, 0xab, 0xab, 0xba, 0x01, 0x07],
                &[0xba, 0xdd, 0xad, 0x00],
            ],
        );
    }

    #[test]
    fn ready() {
        // Pass-through data keeps the read chunking, so compare the byte stream.
        let cfg = json!({"type": "ready", "delimiter": "READY"});
        let input: &[u8] = b"bad data then RREAREADY!!!!!!";
        let chunked = write(&mut *build(cfg.clone()), &[input]).concat();
        let bytes: Vec<&[u8]> = input.chunks(1).collect();
        let bytewise = write(&mut *build(cfg), &bytes).concat();
        assert_eq!(chunked, b"!!!!!!");
        assert_eq!(bytewise, chunked);
    }

    #[test]
    fn regex() {
        same_bytewise(
            json!({"type": "regex", "regex": "[\\r\\n]+"}),
            &[
                b"I love robots\r\nEach ",
                b"and Every One\r",
                b"even you!",
                "\nThe angry red robot \u{e9}\u{1F916}".as_bytes(),
            ],
        );
        same_bytewise(
            json!({"type": "regex", "regex": "\\r\\n|\\n"}),
            &[
                b"This could be\na poem ",
                b"or prose\r\nsent from a robot\r\n",
            ],
        );
    }

    #[test]
    fn start_end() {
        same_bytewise(
            json!({"type": "startEnd", "startDelimiter": [7, 7], "endDelimiter": [8, 8]}),
            &[&[1, 2, 3, 7], &[7, 2, 3, 8], &[8, 7, 7, 7, 9, 8, 8, 8]],
        );
        same_bytewise(
            json!({"type": "startEnd", "startDelimiter": "\u{2}", "endDelimiter": "\u{3}",
                   "includeStartDelimiter": true, "includeEndDelimiter": true}),
            &[b"\x02\x03\x02Hello\x03\x02World\x03\x02open"],
        );
    }

    #[test]
    fn slip() {
        same_bytewise(
            json!({
                "type": "slip",
                "START": 0xab, "ESC": 0xcd, "END": 0xbc,
                "ESC_START": 0xac, "ESC_ESC": 0xce, "ESC_END": 0xbd,
            }),
            &[
                &[0xab, 0x01, 0xcd, 0x02, 0xbc],
                &[0xab, 0xcd, 0xac, 0xbc, 0x55],
                &[0xab, 0x01, 0xcd],
            ],
        );
        same_bytewise(
            json!({"type": "slip"}),
            &[&[0x01, 0xc0, 0xdb, 0xdc, 0xc0, 0xdb, 0xdd, 0xc0, 0xc0, 0x09]],
        );
    }

    #[test]
    fn cctalk() {
        same_bytewise(
            json!({"type": "ccTalk"}),
            &[&[
                2, 2, 1, 254, 1, 1, 217, 2, 0, 1, 254, 217, 2, 2, 1, 251, 1, 1, 217,
            ]],
        );
    }
}

/// Regression tests: per-byte work and buffered memory stay bounded, and option
/// typos are errors.
mod bounded {
    use super::*;

    /// Push `data` one byte at a time; returns the frames and the elapsed time.
    fn bytewise(f: &mut dyn Framer, data: &[u8]) -> (Vec<Vec<u8>>, Duration) {
        let t = now();
        let mut out = Vec::new();
        for b in data.chunks(1) {
            f.push(b, t, &mut out);
        }
        (out, t.elapsed())
    }

    // The old framers rescanned the whole buffer on every push: 128 KiB of
    // marker-less input took 15 s (startEnd) and 166 s (regex) in release.
    const LIMIT: Duration = Duration::from_secs(5);

    #[test]
    fn start_end_without_markers_is_linear() {
        let mut p =
            build(json!({"type": "startEnd", "startDelimiter": "<<", "endDelimiter": ">>"}));
        let (out, took) = bytewise(&mut *p, &vec![b'x'; 1 << 20]);
        assert!(out.is_empty());
        assert!(took < LIMIT, "1 MiB of garbage took {took:?}");
    }

    #[test]
    fn start_end_long_open_frame_is_linear() {
        let mut p = build(json!({
            "type": "startEnd", "startDelimiter": "<<", "endDelimiter": ">>",
            "maxBufferSize": 4 << 20,
        }));
        let mut data = b"junk<<".to_vec();
        data.extend(vec![b'x'; 1 << 20]);
        data.extend(b">><<a>");
        let (out, took) = bytewise(&mut *p, &data);
        assert_eq!(out, vec![vec![b'x'; 1 << 20]]);
        assert!(took < LIMIT, "1 MiB open frame took {took:?}");
        assert_eq!(p.flush(), Some(b"<<a>".to_vec()));
    }

    #[test]
    fn start_end_markers_split_across_pushes_after_garbage() {
        same_bytewise(
            json!({"type": "startEnd", "startDelimiter": "<<<", "endDelimiter": ">>>"}),
            &[b"<<x<", b"<<a>>", b">b<<", b"<c>>>>>", b"<<<"],
        );
    }

    #[test]
    fn regex_without_matches_is_linear() {
        let mut p = build(json!({"type": "regex", "regex": "\\r\\n"}));
        let (out, took) = bytewise(&mut *p, &vec![b'x'; 128 << 10]);
        // Only the maxBufferSize cuts.
        assert_eq!(out.len(), 2);
        assert!(took < LIMIT, "128 KiB without a match took {took:?}");
    }

    #[test]
    fn regex_match_spanning_pushes_is_found() {
        same_bytewise(
            json!({"type": "regex", "regex": "END[0-9]+;"}),
            &[b"aaaEND1", b"23;bbbEN", b"D4;", b"ccc"],
        );
        // Longer than REGEX_LOOKBACK and fed byte by byte: found by a doubling rescan.
        let long = format!("<{}>", "-".repeat(3 * REGEX_LOOKBACK));
        let cfg = json!({"type": "regex", "regex": "<-+>"});
        let mut data = b"head".to_vec();
        data.extend(long.as_bytes());
        data.extend(b"tail");
        data.extend(vec![b'.'; 8 * REGEX_LOOKBACK]);
        let (out, _) = bytewise(&mut *build(cfg), &data);
        assert_eq!(out, frames(&[b"head"]));
    }

    #[test]
    fn packet_length_huge_offset_is_rejected_not_a_panic() {
        assert!(rejects(
            json!({"type": "packetLength", "lengthOffset": u64::MAX})
        ));
        assert!(rejects(
            json!({"type": "packetLength", "lengthOffset": 70000})
        ));
        assert!(rejects(json!({
            "type": "packetLength", "lengthOffset": 10, "maxBufferSize": 8,
        })));
    }

    #[test]
    fn packet_length_buffer_is_bounded() {
        let mut p = build(json!({
            "type": "packetLength", "packetOverhead": u64::MAX, "maxLen": u64::MAX,
            "maxBufferSize": 1024,
        }));
        let mut data = vec![0xaa, 0xff];
        data.extend(vec![0u8; 4096]);
        let out = write(&mut *p, &[&data]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].len(), 1024);
        assert!(
            p.flush().is_none(),
            "bytes after the cut are outside a packet"
        );
    }

    #[test]
    fn unknown_fields_are_rejected() {
        assert!(rejects(json!({"type": "readline", "delimter": "\r\n"})));
        assert!(rejects(
            json!({"type": "byteLength", "length": 4, "lenght": 4})
        ));
        assert!(rejects(json!({"type": "slip", "ESCAPE": 1})));
        assert!(rejects(json!({"type": "packetLength", "maxLength": 4})));
        // Node names and aliases keep working.
        build(json!({"type": "interByteTimeout", "interval": 5}));
        build(json!({"type": "ccTalk", "maxDelayBetweenBytesMs": 5}));
        build(json!({"type": "slip", "ESC_END": 1, "escEsc": 2}));
        build(json!({"type": "readline", "encoding": "utf8"}));
    }
}
