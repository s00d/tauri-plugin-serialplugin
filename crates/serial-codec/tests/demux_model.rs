//! Model test for [`ExchangeDemux`]: echo, reply lines, finals and URCs (single,
//! two-line `+CMT`, multi-line `+CUSD`) in any order and read chunking, overlong
//! lines and resets. After every step the live URCs equal the model's and the phase
//! follows the echo.

use crate::at::finals;
use crate::common::*;
use serial_codec::at::{AtResultFormat, ExchangeDemux, ExchangePhase, MAX_DEMUX_LINE};

fn format() -> impl Strategy<Value = AtResultFormat> {
    select(vec![AtResultFormat::Verbose, AtResultFormat::Numeric])
}

const CMD: &str = "AT+CSQ";
/// Reply lines of `AT+CSQ`: its own header and text without a URC shape.
const BODY: [&str; 4] = [
    "+CSQ: 20,99",
    "+CSQ: 31,0",
    "Manufacturer ACME",
    "+7 900 123 45 67",
];
/// Unsolicited lines: foreign `+NAME:` / `^NAME:` headers and `RING`.
const URCS: [&str; 6] = [
    "+CREG: 1",
    "+CMTI: \"SM\",3",
    "RING",
    "^MODE: 3,8",
    "+CGEV: NW DETACH",
    "+CUSD: 0,\"Balance 5\",15",
];
const MENU: [&str; 4] = ["1 Balance", "2 Bonus", "3 Exit", "Select:"];

#[derive(Debug, Clone)]
enum Step {
    Echo(bool),
    Body(usize),
    Urc(usize),
    /// Two-line `+CMT` URC: header plus its PDU line.
    Cmt(String),
    Final(usize),
    /// `+CUSD` whose quoted text spans lines.
    Cusd(u8, Vec<usize>),
    /// A line of `MAX_DEMUX_LINE + extra` bytes (extra 0 or 1) pushed without its line
    /// end, optionally behind a complete URC line in the same read.
    Long {
        extra: usize,
        behind: bool,
    },
    /// Part of a URC line, then `reset()`.
    ResetMidLine(usize, usize),
    Reset,
}

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        2 => any::<bool>().prop_map(Step::Echo),
        3 => (0..BODY.len()).prop_map(Step::Body),
        4 => (0..URCS.len()).prop_map(Step::Urc),
        1 => "[0-9A-F]{10,40}".prop_map(Step::Cmt),
        2 => (0..finals().len()).prop_map(Step::Final),
        2 => (0u8..3, vec(0..MENU.len(), 0..4)).prop_map(|(m, l)| Step::Cusd(m, l)),
        1 => (0usize..2, any::<bool>()).prop_map(|(extra, behind)| Step::Long { extra, behind }),
        1 => (0..URCS.len(), 1usize..8).prop_map(|(u, cut)| Step::ResetMidLine(u, cut)),
        1 => Just(Step::Reset),
    ]
}

/// Push `bytes` in pieces whose sizes cycle through `sizes`; collect the live URCs.
fn push_split(d: &mut ExchangeDemux, bytes: &[u8], sizes: &[usize], at: &mut usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = bytes;
    while !rest.is_empty() {
        let n = sizes[*at % sizes.len()].clamp(1, rest.len());
        *at += 1;
        out.extend(d.process_chunk(&rest[..n]));
        rest = &rest[n..];
    }
    out
}

fn long_line(len: usize) -> String {
    let head = "+CREG: ";
    format!("{head}{}", "x".repeat(len - head.len()))
}

/// The model: echo, reply, finals and URC lines (single, two-line `+CMT`, multi-line
/// `+CUSD`) in any order and any read chunking; overlong lines; resets. After every
/// step the live URCs are exactly the model's and the phase follows the echo.
#[test]
fn demux_matches_model() {
    run_model(
        128,
        (format(), vec(step(), 0..30), vec(1usize..12, 1..5)),
        |(fmt, steps, sizes)| {
            let mut d = ExchangeDemux::with_format(CMD, &[], fmt);
            let mut echo_seen = false;
            let mut at = 0;
            let finals = finals();
            for s in &steps {
                let (got, want) = match s {
                    Step::Echo(lower) => {
                        let cmd = if *lower {
                            CMD.to_lowercase()
                        } else {
                            CMD.to_string()
                        };
                        echo_seen = true;
                        (
                            push_split(&mut d, format!("{cmd}\r\r\n").as_bytes(), &sizes, &mut at),
                            vec![],
                        )
                    }
                    Step::Body(i) => (
                        push_split(
                            &mut d,
                            format!("{}\r\n", BODY[*i]).as_bytes(),
                            &sizes,
                            &mut at,
                        ),
                        vec![],
                    ),
                    Step::Urc(i) => (
                        push_split(
                            &mut d,
                            format!("\r\n{}\r\n", URCS[*i]).as_bytes(),
                            &sizes,
                            &mut at,
                        ),
                        vec![URCS[*i].to_string()],
                    ),
                    Step::Cmt(pdu) => {
                        let head = format!("+CMT: ,{}", pdu.len() / 2);
                        (
                            push_split(
                                &mut d,
                                format!("{head}\r\n{pdu}\r\n").as_bytes(),
                                &sizes,
                                &mut at,
                            ),
                            vec![head, pdu.clone()],
                        )
                    }
                    Step::Final(i) => (
                        push_split(
                            &mut d,
                            format!("\r\n{}\r\n", finals[*i].0).as_bytes(),
                            &sizes,
                            &mut at,
                        ),
                        vec![],
                    ),
                    Step::Cusd(m, lines) => {
                        let mut text = format!("+CUSD: {m},\"Menu");
                        for l in lines {
                            text.push_str("\r\n");
                            text.push_str(MENU[*l]);
                        }
                        text.push_str("\",15");
                        (
                            push_split(&mut d, format!("{text}\r\n").as_bytes(), &sizes, &mut at),
                            vec![text],
                        )
                    }
                    Step::Long { extra, behind } => {
                        let line = long_line(MAX_DEMUX_LINE + extra);
                        let mut first = Vec::new();
                        let mut want = Vec::new();
                        if *behind {
                            first.extend_from_slice(b"+CMTI: \"ME\",1\r\n");
                            want.push("+CMTI: \"ME\",1".to_string());
                        }
                        first.extend_from_slice(line.as_bytes());
                        let mut got = d.process_chunk(&first);
                        // The rest of the line arrives with its end in the next read.
                        got.extend(d.process_chunk(b"2\r\n"));
                        // Up to the limit the line waits for its end; beyond it, it is
                        // dropped up to its line end.
                        if *extra == 0 {
                            want.push(format!("{line}2"));
                        }
                        (got, want)
                    }
                    Step::ResetMidLine(i, cut) => {
                        let line = URCS[*i].as_bytes();
                        let got = d.process_chunk(&line[..(*cut).min(line.len() - 1)]);
                        d.reset();
                        echo_seen = false;
                        (got, vec![])
                    }
                    Step::Reset => {
                        d.reset();
                        echo_seen = false;
                        (vec![], vec![])
                    }
                };
                prop_assert_eq!(got, want, "step {:?}", s);
                let phase = if echo_seen {
                    ExchangePhase::CollectingSolicited
                } else {
                    ExchangePhase::AwaitingFirstLine
                };
                prop_assert_eq!(d.phase(), phase, "step {:?}", s);
            }
            Ok(())
        },
    );
}
