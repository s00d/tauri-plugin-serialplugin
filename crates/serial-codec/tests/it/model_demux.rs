//! Model: AT exchanges.
//!
//! proptest draws a command and what its modem sends in any order: the echo, reply lines, URCs
//! (one line, two lines with a PDU, a multi-line `+CUSD` menu, glued to a final result), stored
//! SMS with misleading text, final results in any case, `CONNECT`, the SMS prompt, overlong
//! lines, resets. The model classifies every logical line from the documented rules; it must
//! agree with the live demux and the completion checks after every step, and with the one-shot
//! parser about the whole reply. Byte soup covers "any input, any reads".

use crate::common::*;
use serial_codec::at::*;
use AtResultFormat::{Numeric, Verbose};

fn format() -> impl Strategy<Value = AtResultFormat> {
    select(vec![Verbose, Numeric])
}

/// V.250 / 27.007 / 27.005 final results, and `SEND OK` / `SEND FAIL` of TCP stacks.
fn finals() -> Vec<(&'static str, ExchangeMatch)> {
    use ExchangeMatch as M;
    vec![
        ("OK", M::Ok),
        ("ERROR", M::Error),
        ("+CME ERROR: 10", M::CmeError { code: Some(10) }),
        ("+CMS ERROR: 500", M::CmsError { code: Some(500) }),
        (
            "+CME ERROR: SIM not inserted",
            M::CmeError { code: Some(10) },
        ),
        ("+CMS ERROR: no such text", M::CmsError { code: None }),
        ("NO CARRIER", M::NoCarrier),
        ("BUSY", M::Busy),
        ("NO ANSWER", M::NoAnswer),
        ("NO DIALTONE", M::NoDialtone),
        ("SEND OK", M::SendOk),
        ("SEND FAIL", M::SendFail),
    ]
}

/// `ATV0` result codes (V.250 §6.3.2); other digits are no final result.
fn numeric(line: &str) -> Option<ExchangeMatch> {
    use ExchangeMatch as M;
    Some(match line {
        "0" => M::Ok,
        "3" => M::NoCarrier,
        "4" => M::Error,
        "6" => M::NoDialtone,
        "7" => M::Busy,
        "8" => M::NoAnswer,
        _ => return None,
    })
}

/// Commands, the headers their reply lines have, and reply lines (some of them deceptive).
#[derive(Debug, Clone, Copy)]
struct Cmd(
    &'static str,
    &'static [&'static str],
    &'static [&'static str],
);

const CMDS: &[Cmd] = &[
    Cmd(
        "AT+CSQ",
        &["+CSQ:"],
        &[
            "+CSQ: 20,99",
            "Manufacturer ACME",
            "+7 900 123 45 67",
            "+Cmti: 1",
            "+1AB: 1",
            "CMTI: 1",
        ],
    ),
    Cmd("AT^SYSCFG?", &["^SYSCFG:"], &["^SYSCFG: 2,2,3FFFFFFF,1,1"]),
    Cmd(
        "AT+COPS?;+CREG?",
        &["+COPS:", "+CREG:"],
        &["+COPS: 0,0,\"Op\",7", "+CREG: 0,1"],
    ),
    Cmd("ATE0V1+CGMI", &["+CGMI:"], &["+CGMI: ACME", "ACME"]),
    Cmd("AT+CLAC", &["+CLAC:"], &["+CGMI", "+CGMM", "+CSQ"]),
    Cmd("AT+CPIN?", &["+CPIN:"], &["+CPIN: NOT READY"]),
    Cmd("AT+IFC=?", &["+IFC:"], &["+IFC (0,1,2)", "+IFC: 2,2"]),
    Cmd("AT+CMGL=\"ALL\"", &["+CMGL:"], &["Storage busy"]),
    Cmd("AT+CMGR=1", &["+CMGR:"], &["+CMGR: 0,,5x"]),
    Cmd("AT+CUSD=1,\"*100#\",15", &["+CUSD:"], &["Please wait"]),
];

/// Unsolicited lines without a data line (a text-mode `+CDS` carries its fields itself).
const URCS: &[&str] = &[
    "+CREG: 1",
    "+CMTI: \"SM\",3",
    "RING",
    "^MODE: 3,8",
    "+CGEV: NW DETACH",
    "+CUSD: 0,\"Balance 5\",15",
    "+CDS: 6,12,\"+10000000000\",145,\"24/01/01,10:00:00+00\",\"24/01/01,10:00:09+00\",0",
    "+CDS:",
];
/// Headers whose next line is one line of data.
const DATA_HEADERS: &[&str] = &[
    "+CMT: ,23",
    "+CMT: \"+10000000000\",,\"24/01/01,10:00:00+00\"",
    "+CBM: 88",
    "+CDS: 25",
];
/// Headers of stored messages: more lines of text may follow.
const STORED: &[&str] = &[
    "+CMGL: 1,1,,26",
    "+CMGL: 2,\"REC READ\",\"+10000000000\",,\"24/01/01,10:00:00+00\"",
    "+CMGR: \"REC READ\",\"+10000000000\",,\"24/01/01,10:00:00+00\"",
];
/// SMS text that looks like something else.
const TEXT: &[&str] = &[
    "07910000000000F0040B910000000000F000004210101000000004D4F29C0E",
    "hello",
    "ok",
    "busy",
    "5",
    "at home",
    "ATM closed",
    "*100#",
    "+7 900 000 00 00",
    "#tag",
    "$5 off",
    "%done",
];
const MENU: &[&str] = &["1 Balance", "2 Bonus", "3 Exit", "Select:"];
const INTERMEDIATE: &[&str] = &["CONNECT", "CONNECT 9600", ">"];

#[derive(Debug, Clone)]
enum Step {
    Echo(bool),
    Body(usize),
    Urc(usize),
    /// Header and its data line.
    Data(usize, usize),
    /// Stored message: header and lines of text.
    Stored(usize, Vec<usize>),
    /// Final result, lower case or not.
    Final(usize, bool),
    /// `ATV0` code (any digit), ended by CR alone or CR LF.
    Digit(u8, bool),
    /// Final result and URC without a line end between them.
    Glued(usize, usize),
    /// `+CUSD` whose quoted text spans lines ended by CR LF, LF or CR.
    Cusd(u8, Vec<usize>, usize),
    Intermediate(usize),
    /// A line of `MAX_DEMUX_LINE + extra` bytes read without its line end, optionally behind
    /// a complete URC line in the same read.
    Long {
        extra: usize,
        behind: bool,
    },
    /// `\r\n> ` without a line end, which comes later.
    Prompt,
    /// Part of a URC line (or nothing), then a new exchange.
    Reset(usize, usize),
}

/// What a modem sends in one exchange.
fn reply_step() -> impl Strategy<Value = Step> {
    prop_oneof![
        3 => any::<bool>().prop_map(Step::Echo),
        4 => (0usize..6).prop_map(Step::Body),
        4 => (0..URCS.len()).prop_map(Step::Urc),
        2 => (0..DATA_HEADERS.len(), 0..TEXT.len()).prop_map(|(h, d)| Step::Data(h, d)),
        3 => (0..STORED.len(), vec(0..TEXT.len(), 1..4)).prop_map(|(h, t)| Step::Stored(h, t)),
        3 => (0..finals().len(), any::<bool>()).prop_map(|(f, l)| Step::Final(f, l)),
        2 => (0u8..10, any::<bool>()).prop_map(|(d, cr)| Step::Digit(d, cr)),
        1 => (0usize..2, 0..GLUED.len()).prop_map(|(f, u)| Step::Glued(f, u)),
        2 => (0u8..3, vec(0..MENU.len(), 0..4), 0usize..3).prop_map(|(m, l, e)| Step::Cusd(m, l, e)),
        1 => (0..INTERMEDIATE.len()).prop_map(Step::Intermediate),
    ]
}

/// Kinds of logical lines.
#[derive(Debug, Clone, PartialEq)]
enum Kind {
    Echo,
    Body,
    Urc,
    Final(ExchangeMatch),
}

/// The documented classification of one exchange's lines.
struct Model {
    cmd: Cmd,
    format: AtResultFormat,
    echo_seen: bool,
    /// The next line is data of a header of this kind; `true`: more text may follow it.
    data: Option<(Kind, bool)>,
    in_text: bool,
    /// Completion state without and with intermediate results.
    last: [Option<ExchangeMatch>; 2],
    lines: Vec<(String, Kind)>,
}

impl Model {
    fn new(cmd: Cmd, format: AtResultFormat) -> Self {
        Self {
            cmd,
            format,
            echo_seen: false,
            data: None,
            in_text: false,
            last: [None, None],
            lines: Vec::new(),
        }
    }

    /// Any case outside SMS data, exact inside it.
    fn final_of(&self, line: &str, exact: bool) -> Option<ExchangeMatch> {
        finals()
            .into_iter()
            .find(|(f, _)| *f == line || (!exact && f.eq_ignore_ascii_case(line)))
            .map(|(_, m)| m)
            .or_else(|| numeric(line).filter(|_| self.format == Numeric))
    }

    fn line(&mut self, line: &str) {
        let in_data = self.data.is_some() || self.in_text;
        let kind = if let Some(m) = self.final_of(line, in_data) {
            (self.data, self.in_text) = (None, false);
            Kind::Final(m)
        } else if let Some((kind, text)) = self.data.take() {
            self.in_text = text;
            kind
        } else if !self.echo_seen && line.eq_ignore_ascii_case(self.cmd.0) {
            self.echo_seen = true;
            Kind::Echo
        } else {
            let urc = regex_lite::Regex::new(r"(?s)^(RING$|[+^#$%*][A-Z][A-Z0-9]*:)").unwrap();
            let own = self.cmd.1.iter().any(|p| line.starts_with(p));
            let kind = if urc.is_match(line) && !own {
                Kind::Urc
            } else {
                Kind::Body
            };
            if DATA_HEADERS.contains(&line) {
                self.data = Some((kind.clone(), false));
            } else if line.starts_with("+CMGL:") || line.starts_with("+CMGR:") {
                self.data = Some((kind.clone(), true));
            }
            kind
        };
        for (i, last) in self.last.iter_mut().enumerate() {
            let connect = INTERMEDIATE.contains(&line) || (self.format == Numeric && line == "1");
            match &kind {
                Kind::Final(m) => *last = Some(m.clone()),
                _ if in_data => {}
                _ if i == 1 && connect => {
                    *last = Some(ExchangeMatch::Intermediate { line: line.into() })
                }
                // Any `AT…` line is the echo of a new command: what ended before it is stale.
                _ if line.get(..2).is_some_and(|p| p.eq_ignore_ascii_case("AT")) => *last = None,
                _ => {}
            }
        }
        self.lines.push((line.to_string(), kind));
    }

    /// The bytes of a reply step; its lines are classified on the way.
    fn step(&mut self, s: &Step) -> String {
        let finals = finals();
        let (bytes, lines): (String, Vec<String>) = match s {
            Step::Echo(lower) => {
                let cmd = if *lower {
                    self.cmd.0.to_lowercase()
                } else {
                    self.cmd.0.to_string()
                };
                (format!("{cmd}\r\r\n"), vec![cmd])
            }
            Step::Body(i) => plain(&[self.cmd.2[i % self.cmd.2.len()]]),
            Step::Urc(i) => plain(&[URCS[*i]]),
            Step::Data(h, d) => plain(&[DATA_HEADERS[*h], TEXT[*d]]),
            Step::Stored(h, t) => {
                let mut l = vec![STORED[*h]];
                l.extend(t.iter().map(|&i| TEXT[i]));
                plain(&l)
            }
            Step::Final(f, lower) => {
                let line = finals[*f].0;
                plain(&[if *lower {
                    line.to_lowercase()
                } else {
                    line.to_string()
                }])
            }
            Step::Digit(d, cr) if *cr && self.format == Numeric => {
                (format!("{d}\r"), vec![d.to_string()])
            }
            Step::Digit(d, _) => plain(&[d.to_string()]),
            Step::Glued(f, u) => (
                format!("\r\n{}{}\r\n", finals[*f].0, GLUED[*u]),
                vec![finals[*f].0.into(), GLUED[*u].into()],
            ),
            Step::Cusd(m, menu, end) => {
                let end = ["\r\n", "\n", "\r"][*end];
                let menu: String = menu.iter().map(|&i| format!("{end}{}", MENU[i])).collect();
                plain(&[format!("+CUSD: {m},\"Menu{menu}\",15")])
            }
            Step::Intermediate(i) => plain(&[INTERMEDIATE[*i]]),
            Step::Long { .. } | Step::Prompt | Step::Reset(..) => unreachable!("not a reply step"),
        };
        for l in lines {
            self.line(&l);
        }
        bytes
    }

    fn urcs_from(&self, at: usize) -> Vec<String> {
        self.lines[at..]
            .iter()
            .filter(|(_, k)| *k == Kind::Urc)
            .map(|(l, _)| l.clone())
            .collect()
    }
}

/// URCs some modems glue to a final result.
const GLUED: &[&str] = &["+CUSD: 0,\"x\",15", "+CMTI: \"SM\",2", "+CGEV: ME DETACH"];

fn plain<T: std::fmt::Display>(lines: &[T]) -> (String, Vec<String>) {
    let bytes = lines.iter().map(|l| format!("\r\n{l}\r\n")).collect();
    (bytes, lines.iter().map(T::to_string).collect())
}

/// The live side of one exchange.
struct Live {
    demux: ExchangeDemux,
    completion: [AtCompletion; 2],
}

impl Live {
    fn new(cmd: Cmd, format: AtResultFormat) -> Self {
        Self {
            demux: ExchangeDemux::with_format(cmd.0, &[], format),
            completion: [false, true].map(|i| AtCompletion::new(i, format)),
        }
    }

    fn read(&mut self, bytes: &[u8]) -> Vec<String> {
        for c in &mut self.completion {
            c.push(bytes);
        }
        self.demux.process_chunk(bytes)
    }

    fn state(&mut self) -> [Option<ExchangeMatch>; 2] {
        let [a, b] = &mut self.completion;
        [a.push(b""), b.push(b"")]
    }
}

#[test]
fn live_exchange_matches_the_model() {
    let step = prop_oneof![
        12 => reply_step(),
        1 => (0usize..2, any::<bool>()).prop_map(|(extra, behind)| Step::Long { extra, behind }),
        1 => Just(Step::Prompt),
        1 => (0..URCS.len(), 0usize..8).prop_map(|(u, cut)| Step::Reset(u, cut)),
    ];
    let strategy = (
        format(),
        select(CMDS.to_vec()),
        vec(step, 0..30),
        vec(1usize..12, 1..5),
    );
    run_model(256, strategy, |(format, cmd, steps, sizes)| {
        prop_assert_eq!(&derive_solicited_prefixes(cmd.0)[..], cmd.1);
        let mut model = Model::new(cmd, format);
        let mut live = Live::new(cmd, format);
        for s in &steps {
            let at = model.lines.len();
            let mut got = Vec::new();
            match s {
                Step::Long { extra, behind } => {
                    let head = "+CREG: ";
                    let line = format!("{head}{}", "x".repeat(MAX_DEMUX_LINE + extra - head.len()));
                    let mut first = line.clone();
                    if *behind {
                        model.line("+CMTI: \"ME\",1");
                        first = format!("+CMTI: \"ME\",1\r\n{line}");
                    }
                    got.extend(live.read(first.as_bytes()));
                    got.extend(live.read(b"2\r\n"));
                    // Up to the limit a line waits for its end; beyond it is dropped.
                    if *extra == 0 {
                        model.line(&format!("{line}2"));
                    }
                }
                Step::Prompt => {
                    got.extend(live.read(b"\r\n> "));
                    let prompt = ExchangeMatch::Intermediate { line: ">".into() };
                    let want = [model.last[0].clone(), Some(prompt)];
                    prop_assert_eq!(live.state(), want, "{:?}", s);
                    got.extend(live.read(b"\r\n"));
                    model.line(">");
                }
                Step::Reset(u, cut) => {
                    let line = URCS[*u].as_bytes();
                    got.extend(live.read(&line[..(*cut).min(line.len() - 1)]));
                    live.demux.reset();
                    live.completion = Live::new(cmd, format).completion;
                    model = Model::new(cmd, format);
                }
                s => {
                    let bytes = model.step(s);
                    for c in chunks(bytes.as_bytes(), &sizes) {
                        got.extend(live.read(c));
                    }
                }
            }
            let at = at.min(model.lines.len());
            prop_assert_eq!(got, model.urcs_from(at), "step {:?}", s);
            prop_assert_eq!(live.state(), model.last.clone(), "step {:?}", s);
            let phase = if model.echo_seen {
                ExchangePhase::CollectingSolicited
            } else {
                ExchangePhase::AwaitingFirstLine
            };
            prop_assert_eq!(live.demux.phase(), phase, "step {:?}", s);
        }
        Ok(())
    });
}

/// The whole reply at once, with an unterminated tail (the start of the next line): the parts,
/// the final result (the last one after the echo) and both completion checks.
#[test]
fn whole_reply_matches_the_model() {
    let strategy = (
        format(),
        select(CMDS.to_vec()),
        vec(reply_step(), 1..16),
        "[A-Z+ ]{0,6}",
    );
    run_model(512, strategy, |(format, cmd, steps, tail)| {
        let mut model = Model::new(cmd, format);
        let raw: String = steps.iter().map(|s| model.step(s)).collect();
        let raw = format!("{raw}{tail}").into_bytes();
        let lines = &model.lines;
        let echo = lines.iter().position(|l| l.1 == Kind::Echo).unwrap_or(0);
        let end = (echo..lines.len())
            .rev()
            .find(|&i| matches!(lines[i].1, Kind::Final(_)));
        let (mut body, mut urc) = (Vec::new(), Vec::new());
        for (line, kind) in &lines[..end.unwrap_or(lines.len())] {
            let own = cmd.1.iter().any(|p| line.starts_with(p));
            match kind {
                Kind::Echo => {}
                Kind::Body => body.push(line.clone()),
                Kind::Urc => urc.push(line.clone()),
                // A result before the final one: call progress and errors of something else.
                Kind::Final(_)
                    if ["NO CARRIER", "BUSY", "NO ANSWER"].contains(&line.as_str())
                        || (line.starts_with('+') && !own) =>
                {
                    urc.push(line.clone())
                }
                Kind::Final(_) => body.push(line.clone()),
            }
        }
        let mut status = AtParseStatus::Unknown;
        if let Some(i) = end {
            urc.extend(lines[i + 1..].iter().map(|l| l.0.clone()));
            if let Kind::Final(m) = &lines[i].1 {
                status = m.to_parse_status();
            }
        }
        let p = parse_at_response(&raw, cmd.0, &[], format);
        let all: Vec<String> = lines.iter().map(|l| l.0.clone()).collect();
        prop_assert_eq!(&p.lines, &all);
        prop_assert_eq!(&p.solicited_body, &body);
        prop_assert_eq!(&p.urc_lines, &urc);
        prop_assert_eq!(p.final_line.as_ref(), end.map(|i| &lines[i].0));
        prop_assert_eq!(p.status, status);
        prop_assert_eq!(at_final_line_complete(&raw, format), model.last[0].clone());
        prop_assert_eq!(
            at_intermediate_line_complete(&raw, format),
            model.last[1].clone()
        );
        // The result the frontend gets carries the same parts.
        let matched = model.last[0].clone().unwrap_or(ExchangeMatch::Idle);
        let r = ExchangeResponse::from_raw(raw.clone(), matched.clone(), cmd.0, &[], format);
        let r = AtCommandResult::from_exchange(cmd.0.into(), r);
        prop_assert_eq!(&r.raw, &raw);
        prop_assert_eq!(
            (r.command.as_str(), r.response.as_bytes()),
            (cmd.0, &raw[..])
        );
        prop_assert_eq!((r.solicited_body, r.urc_lines, r.lines), (body, urc, all));
        prop_assert_eq!((r.status, r.matched, r.timed_out), (status, matched, false));
        Ok(())
    });
}

// ------------------------------------------------------------------ any bytes

/// Byte soup from AT-ish pieces: lines, quotes, CR/LF, broken UTF-8.
fn soup() -> impl Strategy<Value = Vec<u8>> {
    let tokens: Vec<&[u8]> = vec![
        b"AT+CSQ",
        b"at",
        b"\r",
        b"\n",
        b"\r\n",
        b"OK",
        b"ok",
        b"ERROR",
        b"0",
        b"1",
        b"> ",
        b"+CUSD: 1,\"",
        b"\"",
        b",15",
        b"RING",
        b"+CMTI: \"SM\",1",
        b"+CMT: ,5",
        b"0791AB",
        b"+CSQ: 1,2",
        b"+CME ERROR: 3",
        b"CONNECT",
        b"xyz",
        b" ",
        "\u{e9}".as_bytes(),
        b"\xFF",
        b"\xC3",
        b"+CMGR: 0,,5",
        b"+CDS: 6",
        b"OK+CUSD: 2",
    ];
    vec(select(tokens), 0..60).prop_map(|t| t.concat())
}

/// Any bytes in any reads: the same URCs and phase, the incremental completion answers what the
/// one-shot check answers for all bytes so far, nothing panics and every line is made of input.
#[test]
fn any_bytes_in_any_reads() {
    let strategy = (
        prop_oneof![3 => soup(), 1 => vec(any::<u8>(), 0..300)],
        format(),
        prop_oneof![Just("AT+CSQ".to_string()), "[A-Za-z+^&;=?0-9 ]{0,16}"],
        any::<bool>(),
        vec(1usize..16, 1..5),
    );
    run_model(512, strategy, |(data, format, cmd, intermediate, sizes)| {
        let demux = |sizes: &[usize]| {
            let mut d = ExchangeDemux::with_format(&cmd, &[], format);
            let urcs: Vec<String> = chunks(&data, sizes)
                .into_iter()
                .flat_map(|c| d.process_chunk(c))
                .collect();
            (urcs, d.phase())
        };
        let whole = demux(&[data.len().max(1)]);
        prop_assert_eq!(demux(&sizes), whole.clone());
        prop_assert_eq!(demux(&[1]), whole.clone());

        let check: fn(&[u8], AtResultFormat) -> Option<ExchangeMatch> = if intermediate {
            at_intermediate_line_complete
        } else {
            at_final_line_complete
        };
        let mut c = AtCompletion::new(intermediate, format);
        let mut seen = 0;
        for part in chunks(&data, &sizes) {
            seen += part.len();
            prop_assert_eq!(c.push(part), check(&data[..seen], format), "after {}", seen);
        }

        let text = String::from_utf8_lossy(&data);
        let n = text.len();
        let size = |lines: &[String]| lines.iter().map(String::len).sum::<usize>();
        let p = parse_at_response(&data, &cmd, &["+X:".into()], format);
        prop_assert!(size(&p.lines) <= n);
        prop_assert!(p.solicited_body.len() + p.urc_lines.len() <= p.lines.len());
        prop_assert!(split_lines(&text).iter().all(|l| !l.is_empty()));
        prop_assert!(size(&split_lines(&text)) <= n);
        prop_assert!(size(&urc_lines_from_drain(&data, &[])) <= n);
        prop_assert!(size(&whole.0) <= n);
        let _ = derive_solicited_prefixes(&cmd);
        let _ = classify_final_line(&text);
        Ok(())
    });
}

proptest! {
    #![proptest_config(cases(512))]

    /// One line alone: verbose results in any case and spacing, numeric error codes, `ATV0`
    /// digits, intermediate results, extended and numeric line shapes.
    #[test]
    fn result_lines_classify(
        fin in select(finals()), mask in vec(any::<bool>(), 32), code in 0i32..100_000,
        tail in "[a-zA-Z ]{0,10}", shape in "[ ]?[+^#$%*A-Z0-9a]{0,4}[ ]?",
    ) {
        let (line, want) = fin;
        let mixed: String = line.chars().zip(mask.iter().cycle())
            .map(|(c, &up)| if up { c.to_ascii_uppercase() } else { c.to_ascii_lowercase() })
            .collect();
        prop_assert_eq!(classify_final_line(&format!("  {mixed}\r\n")), Some(want));
        for (kind, want) in [
            ("+CME", ExchangeMatch::CmeError { code: Some(code) }),
            ("+CMS", ExchangeMatch::CmsError { code: Some(code) }),
        ] {
            prop_assert_eq!(classify_final_line(&format!("{kind} ERROR: {code}")), Some(want.clone()));
            prop_assert_eq!(classify_final_line(&format!("{kind} ERROR: {code}: {tail}")), Some(want));
        }
        for d in [code, code % 100, code % 10] {
            prop_assert_eq!(classify_numeric_final_line(&d.to_string()), numeric(&d.to_string()));
        }
        let t = shape.trim();
        prop_assert_eq!(is_ext_info_line(&shape), t.starts_with(['+', '^', '#', '$', '%', '*']));
        prop_assert_eq!(is_numeric_result_line(&shape),
            !t.is_empty() && t.len() <= 2 && t.bytes().all(|b| b.is_ascii_digit()));
        for format in [Verbose, Numeric] {
            let one = format == Numeric && t == "1";
            prop_assert_eq!(classify_intermediate_line_with_format(&shape, format), one);
            let connect = format!("CONNECT {tail}x");
            prop_assert!(classify_intermediate_line_with_format(&connect, format));
        }
    }
}
