//! 3GPP 27.010 basic-option frames from other implementations' tests
//! (`fixtures/cmux.json`: zephyr `modem_cmux` tests, Apache-2.0; oFono `unit/test-mux.c`,
//! GPL-2.0-only, used as data only). Every frame is exact bytes, FCS included.

use serde_json::Value;
use serial_codec::cmux::{
    decode_address, encode_frame, encode_uih, fcs, DecodedFrame, Deframer, FLAG, PF, UI, UIH,
};

fn fixtures() -> Vec<Value> {
    serde_json::from_str(include_str!("../fixtures/cmux.json")).unwrap()
}

fn hex(s: &str) -> Vec<u8> {
    let s: String = s.split_whitespace().collect();
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn s<'a>(v: &'a Value, path: &str) -> &'a str {
    v.pointer(path)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("{path} in {v}"))
}

fn option(e: &Value) -> Option<&str> {
    e.pointer("/input/option").and_then(Value::as_str)
}

/// Single complete frames named by the zephyr test (`cmux_frame_*`).
fn named_frames() -> Vec<(String, Vec<u8>)> {
    fixtures()
        .iter()
        .filter(|e| e.pointer("/expected/named").is_some())
        .map(|e| (s(e, "/name").to_string(), hex(s(e, "/input/frame"))))
        .collect()
}

/// Independent bit-by-bit 27.010 FCS (reflected CRC-8, x^8+x^2+x+1, init 0xFF, ones'
/// complement), to check the table-driven one in the crate.
fn reference_fcs(bytes: &[u8]) -> u8 {
    let mut crc = 0xFFu8;
    for &b in bytes {
        crc ^= b;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xE0
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn header_len(frame: &[u8]) -> usize {
    if frame[3] & 0x01 != 0 {
        4
    } else {
        5
    }
}

fn decode_all(stream: &[u8]) -> Vec<DecodedFrame> {
    Deframer::default().feed(stream)
}

fn decode_in_chunks(stream: &[u8], sizes: &[usize]) -> Vec<DecodedFrame> {
    let mut d = Deframer::default();
    let mut out = Vec::new();
    let mut rest = stream;
    for &n in sizes.iter().cycle() {
        if rest.is_empty() {
            break;
        }
        let (chunk, tail) = rest.split_at(n.min(rest.len()));
        out.extend(d.feed(chunk));
        rest = tail;
    }
    out
}

#[test]
fn fcs_of_every_frame_matches_the_vector() {
    let mut checked = 0;
    for (name, frame) in named_frames() {
        let end = header_len(&frame);
        let info_end = frame.len() - 2;
        let covered = if frame[2] & !PF == UI {
            &frame[1..info_end]
        } else {
            &frame[1..end]
        };
        assert_eq!(fcs(covered), frame[info_end], "{name}");
        assert_eq!(reference_fcs(covered), frame[info_end], "{name}");
        checked += 1;
    }
    for e in fixtures().iter().filter(|e| option(e) == Some("basic")) {
        if let Some(want) = e.pointer("/expected/fcs").and_then(Value::as_str) {
            let frame = hex(s(e, "/expected/frame"));
            assert_eq!(
                fcs(&frame[1..header_len(&frame)]),
                hex(want)[0],
                "{}",
                e["name"]
            );
            checked += 1;
        }
    }
    assert!(checked >= 30, "{checked}");
}

#[test]
fn ofono_fill_basic_frames_encode_byte_exact() {
    let mut n = 0;
    for e in fixtures() {
        if e.pointer("/input/op").and_then(Value::as_str) != Some("fill_frame")
            || option(&e) != Some("basic")
        {
            continue;
        }
        let dlci = e.pointer("/input/dlci").and_then(Value::as_u64).unwrap() as u8;
        let control = hex(s(&e, "/input/control"))[0];
        let info = hex(s(&e, "/input/info"));
        let want = hex(s(&e, "/expected/frame"));
        // oFono's address byte for DLCI 1 is 0x07: C/R set.
        assert_eq!(
            encode_frame(dlci, true, control, &info),
            want,
            "{}",
            e["name"]
        );
        let got = decode_all(&want);
        assert_eq!(got.len(), 1, "{}", e["name"]);
        assert_eq!(
            (got[0].dlci, got[0].cr, got[0].kind),
            (dlci, true, control & !PF)
        );
        assert_eq!(got[0].payload, info);
        n += 1;
    }
    assert_eq!(n, 4);
}

#[test]
#[ignore = "advanced option not implemented"]
fn ofono_fill_advanced_frames() {
    // Kept so the vectors are exercised once the advanced option (0x7E flags, 0x7D
    // transparency, FCS over address+control only) exists. Basic-option encoding can
    // never produce these bytes.
    for e in fixtures()
        .iter()
        .filter(|e| option(e) == Some("advanced") && e["input"]["op"] == "fill_frame")
    {
        let dlci = e["input"]["dlci"].as_u64().unwrap() as u8;
        let control = hex(s(e, "/input/control"))[0];
        let info = hex(s(e, "/input/info"));
        assert_eq!(
            encode_frame(dlci, true, control, &info),
            hex(s(e, "/expected/frame")),
            "{}",
            e["name"]
        );
    }
}

#[test]
fn zephyr_frames_decode_and_reencode_byte_exact() {
    for (name, frame) in named_frames() {
        let got = decode_all(&frame);
        assert_eq!(got.len(), 1, "{name}");
        let f = &got[0];
        let (dlci, cr) = decode_address(frame[1]);
        assert_eq!((f.dlci, f.cr, f.kind), (dlci, cr, frame[2] & !PF), "{name}");
        assert_eq!(
            f.payload,
            frame[header_len(&frame)..frame.len() - 2],
            "{name}"
        );
        assert_eq!(
            encode_frame(f.dlci, f.cr, frame[2], &f.payload),
            frame,
            "{name}"
        );
    }
}

#[test]
fn zephyr_frame_kinds_match_their_names() {
    use serial_codec::cmux::{DISC, SABM, UA};
    for (name, frame) in named_frames() {
        let kind = decode_all(&frame)[0].kind;
        let want = if name.ends_with("sabm_cmd") {
            SABM
        } else if name.ends_with("sabm_ack") || name.ends_with("ua_ack") {
            UA
        } else if name.ends_with("disc_cmd") {
            DISC
        } else {
            // MSC / CLD / PSC / FCon / FCoff and data all ride on UIH.
            UIH
        };
        assert_eq!(kind, want, "{name}");
    }
}

#[test]
fn all_frames_back_to_back_with_garbage_in_any_chunking() {
    let frames = named_frames();
    let mut stream = vec![0x00, 0x41, 0x54, 0x0D]; // line noise before the first flag
    let mut want = Vec::new();
    for (i, (_, frame)) in frames.iter().enumerate() {
        match i % 3 {
            // Shared flag: the closing flag of the previous frame opens this one.
            0 if i > 0 => stream.extend_from_slice(&frame[1..]),
            // Idle flags between frames.
            1 => {
                stream.extend_from_slice(&[FLAG, FLAG]);
                stream.extend_from_slice(frame);
            }
            // Garbage between frames.
            _ => {
                stream.extend_from_slice(b"\x13\x37garbage");
                stream.extend_from_slice(frame);
            }
        }
        want.push(decode_all(frame).remove(0));
    }
    assert_eq!(decode_all(&stream), want);
    for sizes in [&[1][..], &[2, 3], &[7, 1, 5], &[64]] {
        assert_eq!(decode_in_chunks(&stream, sizes), want, "chunks {sizes:?}");
    }
}

#[test]
fn ofono_extract_basic_streams() {
    for e in fixtures().iter().filter(|e| {
        e.pointer("/input/op").and_then(Value::as_str) == Some("extract_frames")
            && option(e) == Some("basic")
    }) {
        let stream = hex(s(e, "/input/stream"));
        let want: Vec<Vec<u8>> = e["expected"]["results"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|r| r.get("payload").and_then(Value::as_str).map(hex))
            .collect();
        for sizes in [&[stream.len()][..], &[1], &[4, 8, 1, 9]] {
            let got: Vec<Vec<u8>> = decode_in_chunks(&stream, sizes)
                .into_iter()
                .inspect(|f| assert_eq!((f.dlci, f.kind), (1, UIH)))
                .map(|f| f.payload)
                .collect();
            assert_eq!(got, want, "{} {sizes:?}", e["name"]);
        }
        // Garbage then a frame without its closing flag: nothing yet.
        let mut d = Deframer::default();
        assert!(d.feed(&stream[..12]).is_empty(), "{}", e["name"]);
    }
}

#[test]
#[ignore = "advanced option not implemented"]
fn ofono_extract_advanced_streams() {
    for e in fixtures().iter().filter(|e| {
        e.pointer("/input/op").and_then(Value::as_str) == Some("extract_frames")
            && option(e) == Some("advanced")
    }) {
        let frames = decode_all(&hex(s(e, "/input/stream")));
        assert_eq!(frames.len(), 2, "{}", e["name"]);
    }
}

/// zephyr receive tests: chunks as the test feeds them, payloads concatenated per DLCI.
#[test]
fn zephyr_receive_streams() {
    let mut n = 0;
    for e in fixtures() {
        let (Some(chunks), Some(dlci)) = (
            e.pointer("/input/rx_stream").and_then(Value::as_array),
            e.pointer("/expected/dlci").and_then(Value::as_u64),
        ) else {
            continue;
        };
        let want = hex(s(&e, "/expected/payload"));
        let mut d = Deframer::default();
        let frames: Vec<DecodedFrame> = chunks
            .iter()
            .flat_map(|c| d.feed(&hex(c.as_str().unwrap())))
            .collect();
        let got: Vec<u8> = frames
            .iter()
            .filter(|f| u64::from(f.dlci) == dlci && f.kind == UIH)
            .flat_map(|f| f.payload.clone())
            .collect();
        assert_eq!(got, want, "{}", e["name"]);
        if e["expected"]["first_frame_delivered"] == false {
            // The frame with the bogus 2-octet length (0xFE) is dropped, not waited for.
            assert_eq!(frames.len(), 2, "{}", e["name"]);
        }
        n += 1;
    }
    assert_eq!(n, 5);
}

#[test]
fn zephyr_transmit_vectors() {
    for e in fixtures() {
        match s(&e, "/name") {
            // 52-byte PPP payload in one UIH frame (zephyr MTU 64).
            "test_modem_cmux_flow_control_dlci2" => {
                let steps = e["input"]["steps"].as_array().unwrap();
                let data = hex(s(&steps[1], "/tx_payload/data"));
                let want = hex(s(&e["expected"]["steps"][3], "/tx"));
                assert_eq!(encode_frame(2, true, UIH, &data), want);
                // Our encoder caps info at TX_N1 = 31; the receiver gets the same bytes.
                let split = encode_uih(2, &data);
                let back: Vec<u8> = decode_all(&split)
                    .into_iter()
                    .flat_map(|f| f.payload)
                    .collect();
                assert_eq!(back, data);
            }
            // "ATE0" + "\r" goes out as one 11-byte UIH frame.
            "test_modem_cmux_chain_send" => {
                let len = e["expected"]["tx_len"].as_u64().unwrap() as usize;
                assert_eq!(encode_uih(2, b"ATE0\r").len(), len);
            }
            _ => {}
        }
    }
}

/// DM and UI have no vector in the harvested tests; check them against the reference FCS
/// (UI covers the info field too, 27.010 §5.2.1.6).
#[test]
fn dm_and_ui_frames_follow_the_spec_fcs_rule() {
    use serial_codec::cmux::DM;
    let dm = encode_frame(1, false, DM | PF, &[]);
    assert_eq!(dm[..4], [FLAG, 0x05, 0x1F, 0x01]);
    assert_eq!(dm[4], reference_fcs(&dm[1..4]));
    let ui = encode_frame(1, true, UI, b"AT\r");
    assert_eq!(ui[ui.len() - 2], reference_fcs(&ui[1..ui.len() - 2]));
    let uih = encode_frame(1, true, UIH, b"AT\r");
    assert_eq!(uih[uih.len() - 2], reference_fcs(&uih[1..4]));
    for frame in [dm, ui] {
        let f = decode_all(&frame).remove(0);
        assert_eq!(encode_frame(f.dlci, f.cr, frame[2], &f.payload), frame);
    }
}

#[test]
fn info_longer_than_the_receive_limit_is_dropped_unless_raised() {
    let big = encode_frame(1, true, UIH, &[0x55; 2000]);
    let mut stream = big.clone();
    stream.extend_from_slice(&encode_uih(1, b"OK"));
    let got = decode_all(&stream);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].payload, b"OK");
    let got = Deframer::with_max_info(2000).feed(&stream);
    assert_eq!(got.len(), 2);
    assert_eq!(got[0].payload.len(), 2000);
}
