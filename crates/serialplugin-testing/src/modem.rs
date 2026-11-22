//! A fake AT modem: 27.005 / 27.007 command lines, 27.010 multiplexer.

use crate::mux::{self, Frame, Reader, DISC, DM, PF, SABM, UA, UIH};
use crate::WAIT;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Instant;

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
pub struct ModemLog(Arc<(Mutex<Seen>, Condvar)>);

impl ModemLog {
    fn lock(&self) -> MutexGuard<'_, Seen> {
        self.0 .0.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn note(&self, f: impl FnOnce(&mut Seen)) {
        f(&mut self.lock());
        self.0 .1.notify_all();
    }

    /// Wait until `found` finds something; panics with `what` after [`WAIT`].
    fn wait<T>(&self, what: &str, found: impl Fn(&Seen) -> Option<T>) -> T {
        let deadline = Instant::now() + WAIT;
        let mut seen = self.lock();
        loop {
            if let Some(found) = found(&seen) {
                return found;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                drop(seen);
                panic!("never: {what}");
            }
            let woken = self.0 .1.wait_timeout(seen, left);
            seen = woken.unwrap_or_else(|e| e.into_inner()).0;
        }
    }

    /// Every command line so far.
    pub fn commands(&self) -> Vec<Command> {
        self.lock().commands.clone()
    }

    /// Wait until the modem read `line` (on any channel, in any case).
    pub fn wait_command(&self, line: &str) -> Command {
        self.wait(&format!("the modem read {line}"), |s| {
            let found = |c: &&Command| c.line.eq_ignore_ascii_case(line);
            s.commands.iter().find(found).cloned()
        })
    }

    /// Every frame received while multiplexed.
    pub fn frames(&self) -> Vec<Frame> {
        self.lock().frames.clone()
    }

    /// Wait until a frame satisfies `pred`; returns it.
    pub fn wait_frame(&self, what: &str, pred: impl Fn(&Frame) -> bool) -> Frame {
        self.wait(what, |s| s.frames.iter().find(|f| pred(f)).cloned())
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
///
/// ```
/// use serialplugin_testing::MuxOptions;
///
/// let options = MuxOptions::default().refuse([4]).lost_first_frames(1);
/// assert!(options.answer_cld && options.refuse.contains(&4));
/// ```
#[derive(Debug, Clone)]
#[non_exhaustive]
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

impl MuxOptions {
    /// Set [`answer_cld`](field@Self::answer_cld).
    pub fn answer_cld(mut self, on: bool) -> Self {
        self.answer_cld = on;
        self
    }

    /// Add to [`deaf`](field@Self::deaf).
    pub fn deaf(mut self, dlcis: impl IntoIterator<Item = u8>) -> Self {
        self.deaf.extend(dlcis);
        self
    }

    /// Add to [`refuse`](field@Self::refuse).
    pub fn refuse(mut self, dlcis: impl IntoIterator<Item = u8>) -> Self {
        self.refuse.extend(dlcis);
        self
    }

    /// Set [`lost_first_frames`](field@Self::lost_first_frames).
    pub fn lost_first_frames(mut self, n: usize) -> Self {
        self.lost_first_frames = n;
        self
    }
}

#[derive(Debug, Clone)]
enum Reply {
    /// Body lines, then the final result.
    Lines(Vec<String>, String),
    /// Sent as is.
    Raw(Vec<u8>),
    Silent,
}

/// The modem as a state machine: [`Modem::feed`] takes what the host wrote and returns what
/// the modem sends back. Plug it into [`FakeDevice::modem`](crate::FakeDevice::modem) or
/// [`peer::serve`](crate::peer::serve).
///
/// Commands match in any case (V.250: `at` is `AT`).
///
/// The multiplexer keeps the state of each DLC, as 27.010 asks: a channel carries data from
/// its SABM to its DISC, anything else on it is answered with DM. DISC on DLCI 0 and CLD end
/// the multiplexer: what follows is plain AT again.
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
    /// By the command in upper case.
    replies: HashMap<String, Reply>,
    echo: bool,
    numeric: bool,
    /// `None`: no multiplexer (`AT+CMUX` is an error).
    mux: Option<MuxOptions>,
    log: ModemLog,
    session: Option<Session>,
    lines: HashMap<Option<u8>, Vec<u8>>,
    /// Channels waiting for the PDU of their `AT+CMGS`.
    prompts: HashSet<Option<u8>>,
}

/// A running multiplexer.
#[derive(Debug)]
struct Session {
    reader: Reader,
    /// Frames still to lose.
    lost: usize,
    /// Established DLCs.
    open: HashSet<u8>,
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
            prompts: HashSet::new(),
        };
        modem.reply("AT", &[]).reply("AT+CSQ", &["+CSQ: 20,99"])
    }

    fn on(mut self, command: &str, reply: Reply) -> Self {
        self.replies.insert(command.to_ascii_uppercase(), reply);
        self
    }

    /// `command` answers with `lines`, then OK.
    pub fn reply(self, command: &str, lines: &[&str]) -> Self {
        let lines = lines.iter().map(|l| l.to_string()).collect();
        self.on(command, Reply::Lines(lines, "OK".into()))
    }

    /// `command` answers with the final result `result` (`ERROR`, `+CME ERROR: 10`, …).
    pub fn fails(self, command: &str, result: &str) -> Self {
        self.on(command, Reply::Lines(Vec::new(), result.into()))
    }

    /// `command` answers with exactly `bytes`.
    pub fn raw(self, command: &str, bytes: &[u8]) -> Self {
        self.on(command, Reply::Raw(bytes.to_vec()))
    }

    /// `command` gets no answer.
    pub fn silent(self, command: &str) -> Self {
        self.on(command, Reply::Silent)
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
        let mut out = Vec::new();
        let mut rest = bytes.to_vec();
        // Frames may follow the command that starts the multiplexer, and text the frame
        // that ends it.
        while !rest.is_empty() {
            match &mut self.session {
                None => {
                    let (reply, taken) = self.terminal(None, &rest);
                    out.extend(reply);
                    rest.drain(..taken);
                }
                Some(session) => {
                    // Out of the session, which a frame may end.
                    let mut reader = std::mem::take(&mut session.reader);
                    reader.push(&std::mem::take(&mut rest));
                    while self.session.is_some() {
                        let Some(frame) = reader.next_frame() else {
                            break;
                        };
                        out.extend(self.frame(frame, unread));
                    }
                    match &mut self.session {
                        Some(session) => session.reader = reader,
                        None => rest = reader.into_rest(),
                    }
                }
            }
        }
        out
    }

    fn frame(&mut self, f: Frame, unread: usize) -> Vec<u8> {
        let (Some(options), Some(session)) = (&self.mux, &mut self.session) else {
            return Vec::new();
        };
        if session.lost > 0 {
            session.lost -= 1;
            return Vec::new();
        }
        self.log.note(|s| s.frames.push(f.clone()));
        // Responses of the responder: C/R 1 (27.010 §5.2.1.2).
        let ua = mux::frame(f.dlci, true, UA | PF, &[]);
        let dm = mux::frame(f.dlci, true, DM | PF, &[]);
        match f.kind {
            SABM if options.deaf.contains(&f.dlci) => Vec::new(),
            SABM if options.refuse.contains(&f.dlci) => dm,
            SABM => {
                session.open.insert(f.dlci);
                ua
            }
            // Responses of the host to what the test sent.
            UA | DM => Vec::new(),
            _ if !session.open.contains(&f.dlci) => dm,
            DISC => {
                session.open.remove(&f.dlci);
                self.forget_line(f.dlci);
                if f.dlci == 0 {
                    self.session = None;
                }
                ua
            }
            UIH if f.dlci == 0 && f.payload.first() == Some(&mux::CLD) => {
                if unread > 0 {
                    self.log.note(|s| s.cld_before_ua = true);
                }
                if !options.answer_cld {
                    return Vec::new();
                }
                self.session = None;
                mux::control(mux::CLD & !2, &[])
            }
            UIH if f.dlci != 0 => {
                let (reply, _) = self.terminal(Some(f.dlci), &f.payload);
                // 31 octets per frame: the default N1.
                reply
                    .chunks(31)
                    .flat_map(|c| mux::data(f.dlci, c))
                    .collect()
            }
            _ => Vec::new(),
        }
    }

    /// A channel opened again starts with an empty line and no prompt.
    fn forget_line(&mut self, dlci: u8) {
        self.lines.remove(&Some(dlci));
        self.prompts.remove(&Some(dlci));
    }

    /// Command lines on the port itself (`None`) or on a DLCI: the answer, and how many
    /// bytes were text (the rest comes after the start of the multiplexer).
    fn terminal(&mut self, channel: Option<u8>, bytes: &[u8]) -> (Vec<u8>, usize) {
        let mut out = Vec::new();
        for (i, &b) in bytes.iter().enumerate() {
            if self.prompts.contains(&channel) {
                // Ctrl-Z ends the PDU, ESC drops it.
                match b {
                    0x1A => {
                        self.prompts.remove(&channel);
                        let pdu = std::mem::take(self.lines.entry(channel).or_default());
                        let pdu = String::from_utf8_lossy(&pdu).trim().to_string();
                        self.log.note(|s| s.pdus.push(pdu));
                        out.extend(self.lines_reply(&["+CMGS: 7".into()], "OK"));
                    }
                    0x1B => {
                        self.prompts.remove(&channel);
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
            self.log.note(|s| {
                s.commands.push(Command {
                    dlci: channel,
                    line: command.clone(),
                })
            });
            out.extend(self.answer(channel, &command));
            if channel.is_none() && self.session.is_some() {
                return (out, i + 1);
            }
        }
        (out, bytes.len())
    }

    fn answer(&mut self, channel: Option<u8>, command: &str) -> Vec<u8> {
        let command = command.to_ascii_uppercase();
        if let Some(reply) = self.replies.get(&command).cloned() {
            return match reply {
                Reply::Lines(lines, result) => self.lines_reply(&lines, &result),
                Reply::Raw(bytes) => bytes,
                Reply::Silent => Vec::new(),
            };
        }
        if command.starts_with("AT+CMUX=") && channel.is_none() {
            if let Some(options) = &self.mux {
                self.lines.retain(|channel, _| channel.is_none());
                self.prompts.clear();
                self.session = Some(Session {
                    reader: Reader::default(),
                    lost: options.lost_first_frames,
                    open: HashSet::new(),
                });
                return self.lines_reply(&[], "OK");
            }
        }
        if command.starts_with("AT+CMGS=") {
            self.prompts.insert(channel);
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
