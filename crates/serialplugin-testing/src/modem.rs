//! A fake AT modem: 27.005 / 27.007 command lines, 27.010 multiplexer.

use crate::eventually;
use crate::mux::{self, Frame, Reader, DISC, DM, PF, SABM, UA, UIH};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard};

/// A command line the modem read, without its CR.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Command {
    /// The multiplexer channel it came on; `None` is the port itself.
    pub dlci: Option<u8>,
    /// The line as the host wrote it, surrounding spaces trimmed.
    pub line: String,
}

#[derive(Debug, Default)]
struct Seen {
    commands: Vec<Command>,
    frames: Vec<Frame>,
    pdus: Vec<String>,
    cld_before_ua: bool,
}

/// What a [`Modem`] saw, in order; stays usable after the modem moved into a device.
#[derive(Debug, Clone, Default)]
pub struct ModemLog(Arc<Mutex<Seen>>);

impl ModemLog {
    fn lock(&self) -> MutexGuard<'_, Seen> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Every command line so far.
    pub fn commands(&self) -> Vec<Command> {
        self.lock().commands.clone()
    }

    /// Wait until the modem read `line` (on any channel).
    pub fn wait_command(&self, line: &str) {
        eventually(&format!("the modem read {line}"), || {
            self.lock().commands.iter().any(|c| c.line == line)
        });
    }

    /// Every frame received while multiplexed.
    pub fn frames(&self) -> Vec<Frame> {
        self.lock().frames.clone()
    }

    /// Wait until a frame satisfies `pred`; returns it.
    pub fn wait_frame(&self, what: &str, pred: impl Fn(&Frame) -> bool) -> Frame {
        let mut found = None;
        eventually(what, || {
            found = self.lock().frames.iter().find(|f| pred(f)).cloned();
            found.is_some()
        });
        found.unwrap()
    }

    /// PDUs sent after an `AT+CMGS` prompt and ended by Ctrl-Z.
    pub fn pdus(&self) -> Vec<String> {
        self.lock().pdus.clone()
    }

    /// The host sent CLD before it read the UA to its last DISC. Real modems (Quectel M26)
    /// answer such a CLD late. Only known on a [`FakeDevice`](crate::FakeDevice).
    pub fn cld_before_ua(&self) -> bool {
        self.lock().cld_before_ua
    }
}

/// How the multiplexer behaves; the default answers every request.
#[derive(Debug, Clone)]
pub struct MuxOptions {
    /// Answer CLD and leave the multiplexer; off, CLD gets no answer.
    pub answer_cld: bool,
    /// DLCIs whose SABM gets no answer.
    pub deaf: HashSet<u8>,
    /// DLCIs whose SABM is refused (DM).
    pub refuse: HashSet<u8>,
    /// Frames ignored right after the `AT+CMUX` OK, as a Quectel M26 does when the host
    /// is too quick.
    pub lost_first_frames: usize,
}

impl Default for MuxOptions {
    fn default() -> Self {
        Self {
            answer_cld: true,
            deaf: HashSet::new(),
            refuse: HashSet::new(),
            lost_first_frames: 0,
        }
    }
}

#[derive(Debug, Clone)]
enum Reply {
    /// Body lines, then the final result.
    Lines(Vec<String>, &'static str),
    /// Sent as is.
    Raw(Vec<u8>),
    Silent,
}

/// The modem as a state machine: [`Modem::feed`] takes what the host wrote and returns what
/// the modem sends back. Plug it into [`FakeDevice::modem`](crate::FakeDevice::modem) or
/// [`peer::serve`](crate::peer::serve).
///
/// ```
/// use serialplugin_testing::Modem;
///
/// let mut modem = Modem::new()
///     .reply("AT+CPIN?", &["+CPIN: READY"])
///     .fails("AT+COPS?", "+CME ERROR: 30");
/// let log = modem.log();
/// assert_eq!(modem.feed(b"AT+CPIN?\r"), b"\r\n+CPIN: READY\r\n\r\nOK\r\n");
/// assert_eq!(modem.feed(b"AT+COPS?\r"), b"\r\n+CME ERROR: 30\r\n");
/// assert_eq!(log.commands()[1].line, "AT+COPS?");
/// ```
#[derive(Debug)]
pub struct Modem {
    replies: HashMap<String, Reply>,
    echo: bool,
    numeric: bool,
    /// `None`: no multiplexer (`AT+CMUX` is an error).
    mux: Option<MuxOptions>,
    log: ModemLog,
    /// Open multiplexer and the frames still to lose.
    session: Option<(Reader, usize)>,
    lines: HashMap<Option<u8>, Vec<u8>>,
    /// Waiting for the PDU of `AT+CMGS` on this channel.
    pdu: Option<Option<u8>>,
}

impl Default for Modem {
    fn default() -> Self {
        Self::new()
    }
}

impl Modem {
    /// Answers `AT` with OK, `AT+CSQ` with a reading, `AT+CMGS=…` with the prompt, then
    /// `+CMGS: 7`; anything else with ERROR.
    pub fn new() -> Self {
        let modem = Self {
            replies: HashMap::new(),
            echo: false,
            numeric: false,
            mux: None,
            log: ModemLog::default(),
            session: None,
            lines: HashMap::new(),
            pdu: None,
        };
        modem.reply("AT", &[]).reply("AT+CSQ", &["+CSQ: 20,99"])
    }

    /// `command` answers with `lines`, then OK.
    pub fn reply(mut self, command: &str, lines: &[&str]) -> Self {
        let lines = lines.iter().map(|l| l.to_string()).collect();
        self.replies
            .insert(command.into(), Reply::Lines(lines, "OK"));
        self
    }

    /// `command` answers with the final result `result` (`ERROR`, `+CME ERROR: 10`, …).
    pub fn fails(mut self, command: &str, result: &'static str) -> Self {
        self.replies
            .insert(command.into(), Reply::Lines(Vec::new(), result));
        self
    }

    /// `command` answers with exactly `bytes`.
    pub fn raw(mut self, command: &str, bytes: &[u8]) -> Self {
        self.replies
            .insert(command.into(), Reply::Raw(bytes.to_vec()));
        self
    }

    /// `command` gets no answer.
    pub fn silent(mut self, command: &str) -> Self {
        self.replies.insert(command.into(), Reply::Silent);
        self
    }

    /// Echo command lines (ATE1).
    pub fn echo(mut self) -> Self {
        self.echo = true;
        self
    }

    /// Numeric result codes (ATV0): `0\r` for OK, `4\r` for ERROR.
    pub fn numeric(mut self) -> Self {
        self.numeric = true;
        self
    }

    /// `AT+CMUX…` starts a multiplexer.
    pub fn mux(mut self, options: MuxOptions) -> Self {
        self.mux = Some(options);
        self
    }

    /// The log, also after the modem moved into a device.
    pub fn log(&self) -> ModemLog {
        self.log.clone()
    }

    /// What the modem answers to `bytes` from the host.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<u8> {
        self.feed_with_unread(bytes, 0)
    }

    /// `unread`: bytes sent earlier that the host has not read yet.
    pub(crate) fn feed_with_unread(&mut self, bytes: &[u8], unread: usize) -> Vec<u8> {
        let Some((reader, _)) = &mut self.session else {
            return self.terminal(None, bytes);
        };
        let frames = reader.feed(bytes);
        let mut out = Vec::new();
        for frame in frames {
            out.extend(self.frame(frame, unread));
        }
        out
    }

    fn frame(&mut self, f: Frame, unread: usize) -> Vec<u8> {
        let options = self.mux.clone().unwrap_or_default();
        let Some((_, lost)) = &mut self.session else {
            return Vec::new();
        };
        if *lost > 0 {
            *lost -= 1;
            return Vec::new();
        }
        self.log.lock().frames.push(f.clone());
        match f.kind {
            SABM if options.deaf.contains(&f.dlci) => Vec::new(),
            SABM if options.refuse.contains(&f.dlci) => mux::frame(f.dlci, true, DM | PF, &[]),
            SABM | DISC => mux::frame(f.dlci, true, UA | PF, &[]),
            UIH if f.dlci == 0 && f.payload.first() == Some(&mux::CLD) => {
                if unread > 0 {
                    self.log.lock().cld_before_ua = true;
                }
                if !options.answer_cld {
                    return Vec::new();
                }
                self.session = None;
                mux::frame(0, true, UIH, &[mux::CLD & !2, 1])
            }
            UIH if f.dlci != 0 => {
                let reply = self.terminal(Some(f.dlci), &f.payload);
                // 31 octets per frame: the default N1.
                reply
                    .chunks(31)
                    .flat_map(|c| mux::data(f.dlci, c))
                    .collect()
            }
            _ => Vec::new(),
        }
    }

    /// Command lines on the port itself (`None`) or on a DLCI.
    fn terminal(&mut self, channel: Option<u8>, bytes: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for &b in bytes {
            if self.pdu == Some(channel) {
                // Ctrl-Z ends the PDU, ESC drops it.
                match b {
                    0x1A => {
                        self.pdu = None;
                        let pdu = std::mem::take(self.lines.entry(channel).or_default());
                        let pdu = String::from_utf8_lossy(&pdu).trim().to_string();
                        self.log.lock().pdus.push(pdu);
                        out.extend(self.lines_reply(&["+CMGS: 7".into()], "OK"));
                    }
                    0x1B => {
                        self.pdu = None;
                        self.lines.remove(&channel);
                        out.extend(self.lines_reply(&[], "OK"));
                    }
                    _ => self.lines.entry(channel).or_default().push(b),
                }
                continue;
            }
            if b != b'\r' {
                self.lines.entry(channel).or_default().push(b);
                continue;
            }
            let line = std::mem::take(self.lines.entry(channel).or_default());
            let command = String::from_utf8_lossy(&line).trim().to_string();
            if self.echo {
                out.extend(&line);
                out.push(b'\r');
            }
            self.log.lock().commands.push(Command {
                dlci: channel,
                line: command.clone(),
            });
            out.extend(self.answer(channel, &command));
        }
        out
    }

    fn answer(&mut self, channel: Option<u8>, command: &str) -> Vec<u8> {
        if let Some(reply) = self.replies.get(command).cloned() {
            return match reply {
                Reply::Lines(lines, result) => self.lines_reply(&lines, result),
                Reply::Raw(bytes) => bytes,
                Reply::Silent => Vec::new(),
            };
        }
        if command.starts_with("AT+CMUX=") && channel.is_none() {
            if let Some(options) = &self.mux {
                self.session = Some((Reader::default(), options.lost_first_frames));
                return self.lines_reply(&[], "OK");
            }
        }
        if command.starts_with("AT+CMGS=") {
            self.pdu = Some(channel);
            return b"\r\n> ".to_vec();
        }
        self.lines_reply(&[], "ERROR")
    }

    fn lines_reply(&self, lines: &[String], result: &str) -> Vec<u8> {
        let body: String = lines
            .iter()
            .map(|l| {
                if self.numeric {
                    format!("{l}\r\n")
                } else {
                    format!("\r\n{l}\r\n")
                }
            })
            .collect();
        let result = match (self.numeric, result) {
            (true, "OK") => "0\r".to_string(),
            (true, "ERROR") => "4\r".to_string(),
            _ => format!("\r\n{result}\r\n"),
        };
        (body + &result).into_bytes()
    }
}
