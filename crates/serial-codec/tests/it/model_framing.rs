//! Model: byte-stream framers behind `FramerConfig`.
//!
//! For every parser type proptest draws options and a run of reads, polls (with gaps between
//! them) and ends of stream. Each model is the plain reading of its rule: it keeps the bytes
//! since the last frame (or the whole stream) and searches them from the start every time. After
//! every step the frames and the deadline must equal the model's, so any chunking gives the
//! same frames, no frame is empty and buffers stop at their cap.
//!
//! The long-buffer rules (doubling rescans, linear work) are in `story_framing.rs`, the Node
//! parity cases in `corpus_framing.rs`.

use crate::common::*;
use proptest::option::of;
use serial_codec::framing::*;
use std::time::{Duration, Instant};

// ------------------------------------------------------------------ options

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

fn cap() -> impl Strategy<Value = Option<usize>> {
    of(1usize..20)
}

fn slip_options() -> impl Strategy<Value = SlipOptions> {
    select(vec![
        SlipOptions::default(),
        SlipOptions {
            start: Some(0xab),
            esc: 0xcd,
            end: 0xbc,
            esc_start: Some(0xac),
            esc_esc: 0xce,
            esc_end: 0xbd,
        },
    ])
}

/// Valid packetLength options: small or endless packets, a small buffer.
fn packet_length() -> impl Strategy<Value = FramerConfig> {
    (
        1usize..=2,
        1usize..=2,
        0usize..3,
        prop_oneof![8 => 0usize..6, 1 => Just(usize::MAX)],
        prop_oneof![8 => 0usize..300, 1 => Just(usize::MAX)],
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

/// Options of every parser type. Regex matches are shorter than the lookback.
fn kinds() -> Vec<BoxedStrategy<FramerConfig>> {
    vec![
        (marker(), any::<bool>(), cap())
            .prop_map(
                |(delimiter, include_delimiter, max_buffer_size)| FramerConfig::Delimiter {
                    delimiter,
                    include_delimiter,
                    max_buffer_size,
                },
            )
            .boxed(),
        (of(Just("\r\n".into())), any::<bool>(), cap())
            .prop_map(
                |(delimiter, include_delimiter, max_buffer_size)| FramerConfig::Readline {
                    delimiter,
                    include_delimiter,
                    max_buffer_size,
                    encoding: None,
                },
            )
            .boxed(),
        (1usize..10)
            .prop_map(|length| FramerConfig::ByteLength { length })
            .boxed(),
        (1u64..40, cap())
            .prop_map(
                |(interval_ms, max_buffer_size)| FramerConfig::InterByteTimeout {
                    interval_ms,
                    max_buffer_size,
                },
            )
            .boxed(),
        packet_length().boxed(),
        marker()
            .prop_map(|delimiter| FramerConfig::Ready { delimiter })
            .boxed(),
        (
            select(vec!["\n", "[\r\n]+", "\r\n|\n", "[;,]", "END[0-9];"]),
            cap(),
        )
            .prop_map(|(r, max_buffer_size)| FramerConfig::Regex {
                regex: r.into(),
                max_buffer_size,
                encoding: None,
            })
            .boxed(),
        (marker(), marker(), any::<bool>(), any::<bool>(), cap())
            .prop_map(|(s, e, is, ie, max_buffer_size)| FramerConfig::StartEnd {
                start_delimiter: s,
                end_delimiter: e,
                include_start_delimiter: is,
                include_end_delimiter: ie,
                max_buffer_size,
            })
            .boxed(),
        slip_options().prop_map(FramerConfig::Slip).boxed(),
        of(0u64..60)
            .prop_map(|max_delay_ms| FramerConfig::CcTalk { max_delay_ms })
            .boxed(),
    ]
}

// ------------------------------------------------------------------ models

fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    (from..hay.len()).find(|&i| hay[i..].starts_with(needle))
}

struct Model {
    cfg: FramerConfig,
    /// Bytes since the last frame; for `ready` the bytes before the marker.
    buf: Vec<u8>,
    /// The whole stream since the last end of stream (packetLength, SLIP).
    stream: Vec<u8>,
    /// Frames of `stream` already delivered.
    delivered: usize,
    ready: bool,
    last: Option<Instant>,
}

impl Model {
    fn new(cfg: FramerConfig) -> Self {
        Self {
            cfg,
            buf: Vec::new(),
            stream: Vec::new(),
            delivered: 0,
            ready: false,
            last: None,
        }
    }

    fn push(&mut self, data: &[u8], now: Instant) -> Vec<Vec<u8>> {
        let mut out = self.poll(now);
        match self.cfg.clone() {
            FramerConfig::Delimiter {
                delimiter,
                include_delimiter,
                max_buffer_size,
            } => self.split(
                data,
                &delimiter.to_vec(),
                include_delimiter,
                max_buffer_size,
                &mut out,
            ),
            FramerConfig::Readline {
                delimiter,
                include_delimiter,
                max_buffer_size,
                ..
            } => self.split(
                data,
                &delimiter.unwrap_or("\n".into()).to_vec(),
                include_delimiter,
                max_buffer_size,
                &mut out,
            ),
            FramerConfig::ByteLength { length } => {
                self.buf.extend(data);
                while self.buf.len() >= length {
                    out.push(self.buf.drain(..length).collect());
                }
            }
            FramerConfig::InterByteTimeout {
                max_buffer_size, ..
            } => {
                self.buf.extend(data);
                let max = max_buffer_size.unwrap_or(DEFAULT_MAX_BUFFER);
                while self.buf.len() >= max {
                    out.push(self.buf.drain(..max).collect());
                }
                if !data.is_empty() {
                    self.last = Some(now);
                }
            }
            FramerConfig::PacketLength { .. } => {
                self.stream.extend(data);
                let (frames, _) = packets(&self.cfg, &self.stream);
                out.extend(frames[self.delivered..].to_vec());
                self.delivered = frames.len();
            }
            FramerConfig::Slip(o) => {
                self.stream.extend(data);
                let (frames, _) = slip(&o, &self.stream);
                out.extend(frames[self.delivered..].to_vec());
                self.delivered = frames.len();
            }
            FramerConfig::Ready { delimiter } => {
                let mut data = data.to_vec();
                if !self.ready {
                    self.buf.extend(&data);
                    let marker = delimiter.to_vec();
                    let Some(at) = find(&self.buf, &marker, 0) else {
                        return out;
                    };
                    self.ready = true;
                    data = self.buf.split_off(at + marker.len());
                }
                out.push(data);
            }
            FramerConfig::Regex {
                regex,
                max_buffer_size,
                ..
            } => {
                self.buf.extend(data);
                // Complete characters, undecodable bytes as U+FFFD; `rest` may still become one.
                let (mut text, mut rest) = (String::new(), Vec::new());
                let mut chars = self.buf.utf8_chunks().peekable();
                while let Some(c) = chars.next() {
                    text.push_str(c.valid());
                    match std::str::from_utf8(c.invalid()) {
                        Ok(_) => {}
                        Err(e) if e.error_len().is_none() && chars.peek().is_none() => {
                            rest = c.invalid().to_vec()
                        }
                        Err(_) => text.push('\u{FFFD}'),
                    }
                }
                let regex = regex_lite::Regex::new(&regex).unwrap();
                let mut parts: Vec<&str> = regex.split(&text).collect();
                let mut open = parts.pop().unwrap().as_bytes().to_vec();
                out.extend(parts.iter().map(|p| p.as_bytes().to_vec()));
                if open.len() >= max_buffer_size.unwrap_or(DEFAULT_MAX_BUFFER) {
                    out.push(std::mem::take(&mut open));
                }
                self.buf = [open, rest].concat();
            }
            FramerConfig::StartEnd {
                start_delimiter,
                end_delimiter,
                include_start_delimiter,
                include_end_delimiter,
                max_buffer_size,
            } => {
                self.buf.extend(data);
                let (start, end) = (start_delimiter.to_vec(), end_delimiter.to_vec());
                while let Some(s) = find(&self.buf, &start, 0) {
                    let Some(e) = find(&self.buf, &end, s + start.len()) else {
                        break;
                    };
                    let from = s + if include_start_delimiter {
                        0
                    } else {
                        start.len()
                    };
                    let to = e + if include_end_delimiter { end.len() } else { 0 };
                    out.push(self.buf[from..to].to_vec());
                    self.buf.drain(..e + end.len());
                }
                if self.buf.len() >= max_buffer_size.unwrap_or(DEFAULT_MAX_BUFFER) {
                    // Keep from the last start marker, if a later one than the first byte.
                    let keep = (1..self.buf.len())
                        .rev()
                        .find(|&i| self.buf[i..].starts_with(&start));
                    let keep = keep.unwrap_or(self.buf.len());
                    self.buf.drain(..keep);
                }
            }
            FramerConfig::CcTalk { max_delay_ms } => {
                let max = Duration::from_millis(max_delay_ms.unwrap_or(50));
                if !max.is_zero() && self.last.is_some_and(|l| now - l > max) {
                    self.buf.clear();
                }
                self.last = Some(now);
                self.buf.extend(data);
                while self.buf.len() > 1 && self.buf.len() >= usize::from(self.buf[1]) + 5 {
                    let n = usize::from(self.buf[1]) + 5;
                    out.push(self.buf.drain(..n).collect());
                }
            }
        }
        out.retain(|f| !f.is_empty());
        out
    }

    fn split(
        &mut self,
        data: &[u8],
        d: &[u8],
        include: bool,
        max: Option<usize>,
        out: &mut Vec<Vec<u8>>,
    ) {
        self.buf.extend(data);
        while let Some(at) = find(&self.buf, d, 0) {
            let mut frame: Vec<u8> = self.buf.drain(..at + d.len()).collect();
            if !include {
                frame.truncate(at);
            }
            out.push(frame);
        }
        let max = max.unwrap_or(DEFAULT_MAX_BUFFER);
        while self.buf.len() >= max {
            out.push(self.buf.drain(..max).collect());
        }
    }

    /// Only silence ends a frame by time.
    fn poll(&mut self, now: Instant) -> Vec<Vec<u8>> {
        if self.deadline().is_some_and(|d| now >= d) {
            self.last = None;
            return vec![std::mem::take(&mut self.buf)];
        }
        Vec::new()
    }

    fn deadline(&self) -> Option<Instant> {
        let FramerConfig::InterByteTimeout { interval_ms, .. } = self.cfg else {
            return None;
        };
        let last = self.last.filter(|_| !self.buf.is_empty())?;
        Some(last + Duration::from_millis(interval_ms))
    }

    /// End of stream: the open frame. `ready` keeps only its marker tail and ccTalk drops a
    /// partial frame; packetLength drops bytes outside a packet.
    fn flush(&mut self) -> Vec<Vec<u8>> {
        let rest = match self.cfg.clone() {
            FramerConfig::Ready { .. } => return Vec::new(),
            FramerConfig::CcTalk { .. } => {
                self.buf.clear();
                return Vec::new();
            }
            FramerConfig::PacketLength { .. } => packets(&self.cfg, &self.stream).1,
            FramerConfig::Slip(o) => slip(&o, &self.stream).1,
            _ => std::mem::take(&mut self.buf),
        };
        self.stream.clear();
        self.delivered = 0;
        self.last = None;
        (!rest.is_empty()).then_some(rest).into_iter().collect()
    }
}

/// packetLength over a whole stream: the packets, and the open one at its end. A packet is
/// found by its delimiter (big-endian on the wire) and ends once it is `len + overhead` long,
/// when `len` (little-endian) exceeds `max_len`, or at the buffer cap.
fn packets(cfg: &FramerConfig, stream: &[u8]) -> (Vec<Vec<u8>>, Vec<u8>) {
    let FramerConfig::PacketLength {
        delimiter: Some(delimiter),
        delimiter_bytes: Some(db),
        packet_overhead: Some(overhead),
        length_bytes: Some(lb),
        length_offset: Some(offset),
        max_len: Some(max_len),
        max_buffer_size,
    } = cfg
    else {
        unreachable!("the generator sets every option")
    };
    let delimiters = match delimiter {
        OneOrMany::One(d) => vec![*d],
        OneOrMany::Many(d) => d.clone(),
    };
    let cap = max_buffer_size.unwrap_or(DEFAULT_MAX_BUFFER);
    let value = |b: &[u8]| b.iter().fold(0u64, |a, &x| a << 8 | u64::from(x));
    let mut out = Vec::new();
    let mut at = 0;
    'packet: loop {
        let Some(start) = (at..(stream.len() + 1).saturating_sub(*db))
            .find(|&i| delimiters.contains(&(value(&stream[i..i + db]) as u32)))
        else {
            return (out, Vec::new());
        };
        for n in db + 1..=stream.len() - start {
            let packet = &stream[start..start + n];
            let done = packet.get(*offset..offset + lb).is_some_and(|l| {
                let len = value(&l.iter().rev().copied().collect::<Vec<_>>()) as usize;
                n >= len.saturating_add(*overhead) || len > *max_len
            });
            if done || n >= cap {
                out.push(packet.to_vec());
                at = start + n;
                continue 'packet;
            }
        }
        return (out, stream[start..].to_vec());
    }
}

/// SLIP over a whole stream: the packets and the open one. With a START byte, bytes before it
/// are dropped; an unknown escape ends the packet and its byte opens the next one.
fn slip(o: &SlipOptions, stream: &[u8]) -> (Vec<Vec<u8>>, Vec<u8>) {
    let mut out = vec![Vec::new()];
    let (mut started, mut escaped) = (o.start.is_none(), false);
    for &b in stream {
        if Some(b) == o.start {
            started = true;
        } else if !escaped && b == o.esc {
            escaped = true;
        } else if !escaped && b == o.end {
            out.push(Vec::new());
            started = o.start.is_none();
        } else {
            let mut byte = b;
            if std::mem::take(&mut escaped) {
                byte = match b {
                    b if Some(b) == o.esc_start && o.start.is_some() => o.start.unwrap(),
                    b if b == o.esc_esc => o.esc,
                    b if b == o.esc_end => o.end,
                    b => {
                        out.push(Vec::new());
                        b
                    }
                };
            }
            if started {
                out.last_mut().unwrap().push(byte);
            }
        }
    }
    let open = out.pop().unwrap();
    out.retain(|f| !f.is_empty());
    (out, open)
}

// ------------------------------------------------------------------ runs

#[derive(Debug, Clone)]
enum Act {
    /// A read, `gap` ms after the previous step.
    Push {
        gap: u64,
        data: Vec<u8>,
    },
    Poll {
        gap: u64,
    },
    EndOfStream,
}

fn act() -> impl Strategy<Value = Act> {
    let gap = || prop_oneof![3 => Just(0u64), 2 => 0u64..80];
    prop_oneof![
        8 => (gap(), data(12)).prop_map(|(gap, data)| Act::Push { gap, data }),
        2 => gap().prop_map(|gap| Act::Poll { gap }),
        1 => Just(Act::EndOfStream),
    ]
}

#[test]
fn framers_match_their_models() {
    for kind in kinds() {
        run_model(128, (kind, vec(act(), 0..40)), |(cfg, acts)| {
            // The options as the frontend sends them.
            let json = serde_json::to_value(&cfg).unwrap();
            let mut real = serde_json::from_value::<FramerConfig>(json)
                .unwrap()
                .build()
                .unwrap();
            let mut model = Model::new(cfg);
            prop_assert_eq!(real.flush(), None, "fresh framer");
            let mut now = Instant::now();
            for a in &acts {
                let mut out = Vec::new();
                let want = match a {
                    Act::Push { gap, data } => {
                        now += Duration::from_millis(*gap);
                        real.push(data, now, &mut out);
                        model.push(data, now)
                    }
                    Act::Poll { gap } => {
                        now += Duration::from_millis(*gap);
                        real.poll(now, &mut out);
                        model.poll(now)
                    }
                    Act::EndOfStream => {
                        out.extend(real.flush());
                        model.flush()
                    }
                };
                prop_assert_eq!(out, want, "step {:?}", a);
                prop_assert_eq!(real.deadline(), model.deadline(), "step {:?}", a);
            }
            Ok(())
        });
    }
}

/// Options, the bytes a sender writes, and the frames that must come out.
type Sent = (FramerConfig, Vec<u8>, Vec<Vec<u8>>);

fn sent() -> Vec<BoxedStrategy<Sent>> {
    let payloads = || vec(vec(4u8..0xA0, 1..30), 0..6);
    vec![
        // SLIP: empty packets vanish, with and without the bluetooth quirk and START.
        (
            slip_options(),
            any::<bool>(),
            vec(vec(any::<u8>(), 0..20), 0..6),
        )
            .prop_map(|(o, quirk, packets)| {
                let wire = packets
                    .iter()
                    .flat_map(|p| slip_encode(p, &o, quirk))
                    .collect();
                let want = packets.into_iter().filter(|p| !p.is_empty()).collect();
                (FramerConfig::Slip(o), wire, want)
            })
            .boxed(),
        // packetLength: any length width and offset, junk between the packets.
        (
            1usize..=2,
            1usize..=2,
            0usize..3,
            0usize..3,
            vec((0usize..60, vec(4u8..0xA0, 0..4)), 0..6),
        )
            .prop_map(|(db, lb, extra, trailer, packets)| {
                let offset = db + extra;
                let overhead = offset + lb + trailer;
                let cfg = FramerConfig::PacketLength {
                    delimiter: Some(OneOrMany::One(if db == 1 { 0xAA } else { 0xAAAB })),
                    delimiter_bytes: Some(db),
                    packet_overhead: Some(overhead),
                    length_bytes: Some(lb),
                    length_offset: Some(offset),
                    max_len: Some(1000),
                    max_buffer_size: None,
                };
                let (mut wire, mut want) = (Vec::new(), Vec::new());
                for (len, junk) in packets {
                    wire.extend(junk);
                    let mut p = [0xAA, 0xAB][..db].to_vec();
                    p.resize(offset, 0x11);
                    p.extend(&len.to_le_bytes()[..lb]);
                    p.resize(len + overhead, 0x22);
                    wire.extend(&p);
                    want.push(p);
                }
                (cfg, wire, want)
            })
            .boxed(),
        payloads()
            .prop_map(|frames| {
                let cfg = FramerConfig::Delimiter {
                    delimiter: ByteSeq::Bytes(vec![0xFE, 0xFF]),
                    include_delimiter: false,
                    max_buffer_size: None,
                };
                let wire = frames
                    .iter()
                    .flat_map(|f| [&f[..], &[0xFE, 0xFF]].concat())
                    .collect();
                (cfg, wire, frames)
            })
            .boxed(),
        payloads()
            .prop_map(|frames| {
                let cfg = FramerConfig::StartEnd {
                    start_delimiter: ByteSeq::Bytes(vec![0xFE]),
                    end_delimiter: ByteSeq::Bytes(vec![0xFD, 0xFD]),
                    include_start_delimiter: false,
                    include_end_delimiter: false,
                    max_buffer_size: None,
                };
                let wire = frames
                    .iter()
                    .flat_map(|f| [&[0x01, 0xFE][..], f, &[0xFD, 0xFD]].concat())
                    .collect();
                (cfg, wire, frames)
            })
            .boxed(),
        payloads()
            .prop_map(|frames| {
                let frames: Vec<Vec<u8>> = frames
                    .into_iter()
                    .map(|f| [&[2, f.len() as u8, 1, 254][..], &f, &[217]].concat())
                    .collect();
                let cfg = FramerConfig::CcTalk { max_delay_ms: None };
                (cfg, frames.concat(), frames)
            })
            .boxed(),
    ]
}

#[test]
fn frames_survive_the_wire() {
    for kind in sent() {
        run_model(
            128,
            (kind, vec(1usize..16, 1..4)),
            |((cfg, wire, want), sizes)| {
                let mut f = cfg.build().unwrap();
                let now = Instant::now();
                let mut out = Vec::new();
                for c in chunks(&wire, &sizes) {
                    f.push(c, now, &mut out);
                }
                out.extend(f.flush());
                prop_assert_eq!(out, want);
                Ok(())
            },
        );
    }
}

// ------------------------------------------------------------------ validation

/// Options of every type, valid or not, and whether the documentation allows them.
fn any_options() -> Vec<BoxedStrategy<(FramerConfig, bool)>> {
    let seq = || {
        prop_oneof![
            vec(any::<u8>(), 0..3).prop_map(ByteSeq::Bytes),
            "[a-z]{0,2}".prop_map(ByteSeq::Text)
        ]
    };
    let ok_seq = |s: &ByteSeq| !s.to_vec().is_empty();
    let max = || of(0usize..3);
    vec![
        (seq(), max())
            .prop_map(move |(delimiter, m)| {
                let ok = ok_seq(&delimiter) && m != Some(0);
                let cfg = FramerConfig::Delimiter {
                    delimiter,
                    include_delimiter: true,
                    max_buffer_size: m,
                };
                (cfg, ok)
            })
            .boxed(),
        (of(seq()), max(), of("[a-z0-9]{0,4}"))
            .prop_map(move |(delimiter, m, encoding)| {
                let ok = delimiter.as_ref().is_none_or(ok_seq) && m != Some(0);
                let cfg = FramerConfig::Readline {
                    delimiter,
                    include_delimiter: false,
                    max_buffer_size: m,
                    encoding,
                };
                (cfg, ok)
            })
            .boxed(),
        (0usize..3)
            .prop_map(|length| (FramerConfig::ByteLength { length }, length > 0))
            .boxed(),
        (0u64..3, max())
            .prop_map(|(interval_ms, m)| {
                let cfg = FramerConfig::InterByteTimeout {
                    interval_ms,
                    max_buffer_size: m,
                };
                (cfg, interval_ms > 0 && m != Some(0))
            })
            .boxed(),
        (
            of(prop_oneof![
                any::<u32>().prop_map(OneOrMany::One),
                vec(any::<u32>(), 0..3).prop_map(OneOrMany::Many)
            ]),
            of(0usize..6),
            of(0usize..10),
            of(prop_oneof![8 => 0usize..20, 1 => Just(usize::MAX)]),
            of(0usize..24),
        )
            .prop_map(|(delimiter, db, lb, offset, m)| {
                let (b, l, o, cap) = (
                    db.unwrap_or(1),
                    lb.unwrap_or(1),
                    offset.unwrap_or(1),
                    m.unwrap_or(DEFAULT_MAX_BUFFER),
                );
                let ok = cap > 0
                    && delimiter != Some(OneOrMany::Many(Vec::new()))
                    && (1..=4).contains(&b)
                    && (1..=8).contains(&l)
                    && o >= b
                    && o.checked_add(l).is_some_and(|end| end <= cap);
                let cfg = FramerConfig::PacketLength {
                    delimiter,
                    delimiter_bytes: db,
                    packet_overhead: None,
                    length_bytes: lb,
                    length_offset: offset,
                    max_len: None,
                    max_buffer_size: m,
                };
                (cfg, ok)
            })
            .boxed(),
        seq()
            .prop_map(move |delimiter| {
                let ok = ok_seq(&delimiter);
                (FramerConfig::Ready { delimiter }, ok)
            })
            .boxed(),
        (
            select(vec![
                ("\\n", true),
                ("a|b", true),
                ("[", false),
                ("\\", false),
            ]),
            max(),
        )
            .prop_map(|((regex, ok), m)| {
                let cfg = FramerConfig::Regex {
                    regex: regex.into(),
                    max_buffer_size: m,
                    encoding: None,
                };
                (cfg, ok && m != Some(0))
            })
            .boxed(),
        (seq(), seq(), max())
            .prop_map(move |(s, e, m)| {
                let ok = ok_seq(&s) && ok_seq(&e) && m != Some(0);
                let cfg = FramerConfig::StartEnd {
                    start_delimiter: s,
                    end_delimiter: e,
                    include_start_delimiter: false,
                    include_end_delimiter: false,
                    max_buffer_size: m,
                };
                (cfg, ok)
            })
            .boxed(),
        slip_options()
            .prop_map(|o| (FramerConfig::Slip(o), true))
            .boxed(),
        of(any::<u64>())
            .prop_map(|max_delay_ms| (FramerConfig::CcTalk { max_delay_ms }, true))
            .boxed(),
    ]
}

/// Options build exactly when the documentation allows them, the JSON form reads back, and an
/// unknown option name (a typo) is refused for every type.
#[test]
fn options_are_accepted_exactly_as_documented() {
    for kind in any_options() {
        run_model(128, (kind, "[a-z]{3,8}X"), |((cfg, ok), typo)| {
            prop_assert_eq!(cfg.build().is_ok(), ok);
            let mut json = serde_json::to_value(&cfg).unwrap();
            let back: FramerConfig = serde_json::from_value(json.clone()).unwrap();
            prop_assert_eq!(back, cfg);
            json[typo] = 1.into();
            prop_assert!(serde_json::from_value::<FramerConfig>(json).is_err());
            Ok(())
        });
    }
}
