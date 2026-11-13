//! Typed parsers for 3GPP TS 27.007 / 27.005 information lines and unsolicited result codes.
//!
//! Every parser takes ONE logical line (framing into lines is done elsewhere, e.g. by
//! [`crate::at::split_lines`]) and returns `None` when the line is not that command or is
//! malformed. Prefixes match ASCII case-insensitively; surrounding whitespace / CR LF is ignored.
//! Multi-line responses (`+CMGL`, `+CMGR`, `+CMT`, `+CDS` in PDU mode) parse the header only:
//! the PDU or text body is the next line.
//!
//! [`parse_any`] dispatches the unsolicited forms.
//!
//! ```
//! use serial_codec::urc::{parse_any, AccessTech, RegStatus, Urc};
//!
//! let Some(Urc::Registration(r)) = parse_any("+CEREG: 1,\"1F00\",\"79D903\",7") else { panic!() };
//! assert_eq!(r.n, None); // unsolicited form
//! assert_eq!(r.stat, RegStatus::Home);
//! assert_eq!((r.lac, r.ci, r.act), (Some(0x1F00), Some(0x79D903), Some(AccessTech::Eutran)));
//! ```

use crate::args::{parse_values, split, strip_prefix, Arg};
use serde::Serialize;

fn body(line: &str, prefix: &str) -> Option<Vec<Arg>> {
    strip_prefix(line, prefix).map(split)
}

fn int<T: std::str::FromStr>(a: &[Arg], i: usize) -> Option<T> {
    a.get(i)?.int()
}

fn text(a: &[Arg], i: usize) -> Option<String> {
    a.get(i)?.text().map(str::to_string)
}

// ---------------------------------------------------------------------------------------------
// Network registration: +CREG / +CGREG / +CEREG / +C5GREG (27.007 §7.2, §10.1.20-22, §10.1.68)

/// Which registration command a line came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RegKind {
    /// `+CREG`, circuit switched.
    Creg,
    /// `+CGREG`, GPRS.
    Cgreg,
    /// `+CEREG`, EPS.
    Cereg,
    /// `+C5GREG`, 5GS.
    C5greg,
}

/// `<stat>` of the registration commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RegStatus {
    /// 0: not registered, not searching.
    NotRegistered,
    /// 1: registered, home network.
    Home,
    /// 2: not registered, searching.
    Searching,
    /// 3: registration denied.
    Denied,
    /// 4: unknown (e.g. out of coverage).
    Unknown,
    /// 5: registered, roaming.
    Roaming,
    /// 6: registered for SMS only, home.
    HomeSmsOnly,
    /// 7: registered for SMS only, roaming.
    RoamingSmsOnly,
    /// 8: attached for emergency bearer services only.
    EmergencyOnly,
    /// 9: registered for CSFB not preferred, home.
    HomeCsfbNotPreferred,
    /// 10: registered for CSFB not preferred, roaming.
    RoamingCsfbNotPreferred,
    /// 11: attached for access to RLOS.
    AttachedRlos,
    /// Any other value.
    Other(u8),
}

impl RegStatus {
    pub fn from_code(v: u8) -> Self {
        use RegStatus::*;
        match v {
            0 => NotRegistered,
            1 => Home,
            2 => Searching,
            3 => Denied,
            4 => Unknown,
            5 => Roaming,
            6 => HomeSmsOnly,
            7 => RoamingSmsOnly,
            8 => EmergencyOnly,
            9 => HomeCsfbNotPreferred,
            10 => RoamingCsfbNotPreferred,
            11 => AttachedRlos,
            v => Other(v),
        }
    }

    /// Registered (home or roaming, any service restriction).
    pub fn is_registered(self) -> bool {
        use RegStatus::*;
        matches!(
            self,
            Home | Roaming
                | HomeSmsOnly
                | RoamingSmsOnly
                | HomeCsfbNotPreferred
                | RoamingCsfbNotPreferred
        )
    }
}

/// `<AcT>` access technology (27.007 §7.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AccessTech {
    /// 0 GSM.
    Gsm,
    /// 1 GSM Compact.
    GsmCompact,
    /// 2 UTRAN.
    Utran,
    /// 3 GSM w/EGPRS.
    GsmEgprs,
    /// 4 UTRAN w/HSDPA.
    UtranHsdpa,
    /// 5 UTRAN w/HSUPA.
    UtranHsupa,
    /// 6 UTRAN w/HSDPA and HSUPA.
    UtranHsdpaHsupa,
    /// 7 E-UTRAN.
    Eutran,
    /// 8 EC-GSM-IoT (A/Gb mode).
    EcGsmIot,
    /// 9 E-UTRAN (NB-S1 mode).
    EutranNbS1,
    /// 10 E-UTRA connected to a 5GCN.
    Eutra5gcn,
    /// 11 NR connected to a 5GCN.
    Nr5gcn,
    /// 12 NG-RAN.
    NgRan,
    /// 13 E-UTRA-NR dual connectivity.
    EutraNrDual,
    /// Any other value.
    Other(u8),
}

impl AccessTech {
    pub fn from_code(v: u8) -> Self {
        use AccessTech::*;
        match v {
            0 => Gsm,
            1 => GsmCompact,
            2 => Utran,
            3 => GsmEgprs,
            4 => UtranHsdpa,
            5 => UtranHsupa,
            6 => UtranHsdpaHsupa,
            7 => Eutran,
            8 => EcGsmIot,
            9 => EutranNbS1,
            10 => Eutra5gcn,
            11 => Nr5gcn,
            12 => NgRan,
            13 => EutraNrDual,
            v => Other(v),
        }
    }

    fn from_arg(a: &[Arg], i: usize) -> Option<Self> {
        int::<u8>(a, i).map(Self::from_code)
    }
}

/// A `+CREG` / `+CGREG` / `+CEREG` / `+C5GREG` line, solicited (`<n>,<stat>,...`) or
/// unsolicited (`<stat>,...`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Registration {
    pub kind: RegKind,
    /// `<n>` (URC mode); present only in the read-command response form.
    pub n: Option<u8>,
    pub stat: RegStatus,
    /// `<lac>` (CREG/CGREG) or `<tac>` (CEREG/C5GREG).
    pub lac: Option<u32>,
    /// `<ci>` cell id (NR cell ids are 36 bits).
    pub ci: Option<u64>,
    pub act: Option<AccessTech>,
    /// `<rac>` (+CGREG only).
    pub rac: Option<u8>,
    /// `<Allowed_NSSAI>` (+C5GREG only).
    pub allowed_nssai: Option<String>,
    pub cause_type: Option<u8>,
    pub reject_cause: Option<u32>,
}

impl Registration {
    /// Parses any of the four registration commands.
    pub fn parse(line: &str) -> Option<Self> {
        let (kind, a) = [
            ("+CREG", RegKind::Creg),
            ("+CGREG", RegKind::Cgreg),
            ("+CEREG", RegKind::Cereg),
            ("+C5GREG", RegKind::C5greg),
        ]
        .into_iter()
        .find_map(|(p, k)| body(line, p).map(|a| (k, a)))?;

        // Response form: the second field is an unquoted small number (<stat>). A 3-field line
        // is always `<stat>,<lac>,<ci>`.
        // ponytail: an unquoted 1-2 digit LAC (`+CREG: 1,12,3456,0`) reads as the response form;
        // real modems pad LAC/TAC to 4 hex digits, so this has not been seen.
        let response = a.len() != 3
            && a.get(1)
                .is_some_and(|f| !f.quoted && f.text.len() <= 2 && f.int::<u8>().is_some());
        let (n, a) = if response {
            (Some(int::<u8>(&a, 0)?), &a[1..])
        } else {
            (None, &a[..])
        };
        let stat = RegStatus::from_code(int(a, 0)?);
        let hex = |i: usize| a.get(i).and_then(Arg::hex);
        let mut r = Registration {
            kind,
            n,
            stat,
            lac: hex(1).and_then(|v| u32::try_from(v).ok()),
            ci: hex(2),
            act: AccessTech::from_arg(a, 3),
            rac: None,
            allowed_nssai: None,
            cause_type: None,
            reject_cause: None,
        };
        let cause_at = match kind {
            RegKind::Creg | RegKind::Cereg => 4,
            RegKind::Cgreg => {
                r.rac = hex(4).and_then(|v| u8::try_from(v).ok());
                5
            }
            RegKind::C5greg => {
                r.allowed_nssai = text(a, 5);
                6
            }
        };
        r.cause_type = int(a, cause_at);
        r.reject_cause = int(a, cause_at + 1);
        Some(r)
    }
}

// ---------------------------------------------------------------------------------------------
// Signal quality: +CSQ (§8.5), +CESQ (§8.69)

/// `+CSQ: <rssi>,<ber>` raw values; 99 = not known.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Csq {
    pub rssi: u8,
    pub ber: u8,
}

impl Csq {
    pub fn parse(line: &str) -> Option<Self> {
        let a = body(line, "+CSQ")?;
        Some(Csq {
            rssi: int(&a, 0)?,
            ber: int(&a, 1)?,
        })
    }

    /// RSSI in dBm: 0 -> -113, 31 -> -51 or better; `None` for 99 / out of range.
    pub fn rssi_dbm(&self) -> Option<i32> {
        (self.rssi <= 31).then(|| -113 + 2 * self.rssi as i32)
    }
}

/// `+CESQ: <rxlev>,<ber>,<rscp>,<ecno>,<rsrq>,<rsrp>[,<ss_rsrq>,<ss_rsrp>,<ss_sinr>]` raw
/// values; 99 (rxlev, ber) / 255 (others) = not known.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cesq {
    pub rxlev: u8,
    pub ber: u8,
    pub rscp: u8,
    pub ecno: u8,
    pub rsrq: u8,
    pub rsrp: u8,
    pub ss_rsrq: Option<u8>,
    pub ss_rsrp: Option<u8>,
    pub ss_sinr: Option<u8>,
}

impl Cesq {
    pub fn parse(line: &str) -> Option<Self> {
        let a = body(line, "+CESQ")?;
        Some(Cesq {
            rxlev: int(&a, 0)?,
            ber: int(&a, 1)?,
            rscp: int(&a, 2)?,
            ecno: int(&a, 3)?,
            rsrq: int(&a, 4)?,
            rsrp: int(&a, 5)?,
            ss_rsrq: int(&a, 6),
            ss_rsrp: int(&a, 7),
            ss_sinr: int(&a, 8),
        })
    }

    /// GSM RSSI in dBm (lower bound of the 1 dB step): 0 -> -111 or less, 63 -> -48.
    pub fn rxlev_dbm(&self) -> Option<f64> {
        (self.rxlev <= 63).then(|| -111.0 + self.rxlev as f64)
    }
    /// UTRAN RSCP in dBm: 0 -> -121, 96 -> -25.
    pub fn rscp_dbm(&self) -> Option<f64> {
        (self.rscp <= 96).then(|| -121.0 + self.rscp as f64)
    }
    /// UTRAN Ec/No in dB: 0 -> -24.5, 49 -> 0.
    pub fn ecno_db(&self) -> Option<f64> {
        (self.ecno <= 49).then(|| -24.5 + self.ecno as f64 * 0.5)
    }
    /// E-UTRAN RSRQ in dB: 0 -> -20, 34 -> -3.
    pub fn rsrq_db(&self) -> Option<f64> {
        (self.rsrq <= 34).then(|| -20.0 + self.rsrq as f64 * 0.5)
    }
    /// E-UTRAN RSRP in dBm: 0 -> -141, 97 -> -44.
    pub fn rsrp_dbm(&self) -> Option<f64> {
        (self.rsrp <= 97).then(|| -141.0 + self.rsrp as f64)
    }
}

// ---------------------------------------------------------------------------------------------
// +CPIN (§8.3)

/// `+CPIN: <code>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Cpin {
    Ready,
    SimPin,
    SimPuk,
    PhSimPin,
    PhFsimPin,
    PhFsimPuk,
    SimPin2,
    SimPuk2,
    PhNetPin,
    PhNetPuk,
    PhNetsubPin,
    PhNetsubPuk,
    PhSpPin,
    PhSpPuk,
    PhCorpPin,
    PhCorpPuk,
    /// Any other code, verbatim.
    Other(String),
}

impl Cpin {
    pub fn parse(line: &str) -> Option<Self> {
        let a = body(line, "+CPIN")?;
        let code = a.first()?.text()?.to_string();
        use Cpin::*;
        Some(match code.to_ascii_uppercase().as_str() {
            "READY" => Ready,
            "SIM PIN" => SimPin,
            "SIM PUK" => SimPuk,
            "PH-SIM PIN" => PhSimPin,
            "PH-FSIM PIN" => PhFsimPin,
            "PH-FSIM PUK" => PhFsimPuk,
            "SIM PIN2" => SimPin2,
            "SIM PUK2" => SimPuk2,
            "PH-NET PIN" => PhNetPin,
            "PH-NET PUK" => PhNetPuk,
            "PH-NETSUB PIN" => PhNetsubPin,
            "PH-NETSUB PUK" => PhNetsubPuk,
            "PH-SP PIN" => PhSpPin,
            "PH-SP PUK" => PhSpPuk,
            "PH-CORP PIN" => PhCorpPin,
            "PH-CORP PUK" => PhCorpPuk,
            _ => Other(code),
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Calls: +CLIP (§7.6), +COLP (§7.8), +CCWA (§7.12), +CLCC (§7.18)

/// `+CLIP: <number>,<type>[,<subaddr>,<satype>[,[<alpha>][,<CLI validity>]]]` and
/// `+COLP: <number>,<type>[,<subaddr>,<satype>[,<alpha>]]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallerId {
    pub number: String,
    /// Type of address octet (129 national/unknown, 145 international).
    #[serde(rename = "type")]
    pub number_type: Option<u8>,
    pub subaddr: Option<String>,
    pub satype: Option<u8>,
    pub alpha: Option<String>,
    /// 0 valid, 1 withheld, 2 not available (+CLIP only).
    pub cli_validity: Option<u8>,
}

impl CallerId {
    fn parse_with(line: &str, prefix: &str) -> Option<Self> {
        let a = body(line, prefix)?;
        // 27.007 always quotes <number>; the `AT+CLIP?` read response `<n>,<m>` does not.
        if !a.first()?.quoted {
            return None;
        }
        Some(CallerId {
            number: a.first()?.text.clone(),
            number_type: int(&a, 1),
            subaddr: text(&a, 2),
            satype: int(&a, 3),
            alpha: text(&a, 4),
            cli_validity: int(&a, 5),
        })
    }

    pub fn parse_clip(line: &str) -> Option<Self> {
        Self::parse_with(line, "+CLIP")
    }

    pub fn parse_colp(line: &str) -> Option<Self> {
        Self::parse_with(line, "+COLP").map(|c| CallerId {
            cli_validity: None,
            ..c
        })
    }
}

/// `+CCWA: <number>,<type>,<class>,[<alpha>][,<CLI validity>[,<subaddr>,<satype>[,<priority>]]]`
/// (the call waiting URC; the `+CCWA: <status>,<class>` query response is not this).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallWaiting {
    pub number: String,
    #[serde(rename = "type")]
    pub number_type: Option<u8>,
    pub class: Option<u8>,
    pub alpha: Option<String>,
    pub cli_validity: Option<u8>,
    pub subaddr: Option<String>,
    pub satype: Option<u8>,
    pub priority: Option<u8>,
}

impl CallWaiting {
    pub fn parse(line: &str) -> Option<Self> {
        let a = body(line, "+CCWA")?;
        // `+CCWA: 1,1` (query response) has an unquoted number and no type.
        if !a.first()?.quoted && a.len() < 3 {
            return None;
        }
        Some(CallWaiting {
            number: a[0].text.clone(),
            number_type: int(&a, 1),
            class: int(&a, 2),
            alpha: text(&a, 3),
            cli_validity: int(&a, 4),
            subaddr: text(&a, 5),
            satype: int(&a, 6),
            priority: int(&a, 7),
        })
    }
}

/// `<dir>` of +CLCC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CallDirection {
    MobileOriginated,
    MobileTerminated,
}

/// `<stat>` of +CLCC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CallState {
    Active,
    Held,
    Dialing,
    Alerting,
    Incoming,
    Waiting,
    Other(u8),
}

/// `+CLCC: <ccid>,<dir>,<stat>,<mode>,<mpty>[,<number>,<type>[,<alpha>[,<priority>[,<CLI validity>]]]]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Clcc {
    pub id: u32,
    pub dir: CallDirection,
    pub stat: CallState,
    /// 0 voice, 1 data, 2 fax, 9 unknown, ...
    pub mode: u8,
    pub mpty: bool,
    pub number: Option<String>,
    #[serde(rename = "type")]
    pub number_type: Option<u8>,
    pub alpha: Option<String>,
    pub priority: Option<u8>,
    pub cli_validity: Option<u8>,
}

impl Clcc {
    pub fn parse(line: &str) -> Option<Self> {
        let a = body(line, "+CLCC")?;
        let dir = match int::<u8>(&a, 1)? {
            0 => CallDirection::MobileOriginated,
            1 => CallDirection::MobileTerminated,
            _ => return None,
        };
        let stat = match int::<u8>(&a, 2)? {
            0 => CallState::Active,
            1 => CallState::Held,
            2 => CallState::Dialing,
            3 => CallState::Alerting,
            4 => CallState::Incoming,
            5 => CallState::Waiting,
            v => CallState::Other(v),
        };
        let number = a.get(5).map(|n| n.text.clone());
        let number_type = int(&a, 6);
        // <number> and <type> come as a pair.
        if number.is_some() && number_type.is_none() {
            return None;
        }
        Some(Clcc {
            id: int(&a, 0)?,
            dir,
            stat,
            mode: int(&a, 3)?,
            mpty: int::<u8>(&a, 4)? == 1,
            number,
            number_type,
            alpha: text(&a, 7),
            priority: int(&a, 8),
            cli_validity: int(&a, 9),
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Indicators: +CIEV / +CIND (§8.9, §8.10)

/// `+CIEV: <ind>,<value>`. `<ind>` is the 1-based +CIND index; some modems send the name instead.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Ciev {
    pub index: Option<u32>,
    pub name: Option<String>,
    pub value: i64,
}

impl Ciev {
    pub fn parse(line: &str) -> Option<Self> {
        let a = body(line, "+CIEV")?;
        let ind = a.first()?;
        let value = a.get(1)?.text.parse().ok()?;
        let index = ind.int();
        Some(Ciev {
            index,
            name: if index.is_none() {
                ind.text().map(str::to_string)
            } else {
                None
            },
            value,
        })
    }
}

/// `+CIND: <ind>,<ind>,...` read-command response: current values in +CIND=? order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CindValues {
    pub values: Vec<i64>,
}

impl CindValues {
    pub fn parse(line: &str) -> Option<Self> {
        let a = body(line, "+CIND")?;
        let values = a
            .iter()
            .map(|v| v.text.parse().ok())
            .collect::<Option<Vec<_>>>()?;
        (!values.is_empty()).then_some(CindValues { values })
    }
}

/// One entry of `+CIND: ("name",(range)),...`; `index` is 1-based.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndicatorRange {
    pub index: u32,
    pub name: String,
    pub values: Vec<u32>,
}

/// `+CIND: ("<descr>",(list of supported <ind>s)),...` test-command response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CindRanges {
    pub indicators: Vec<IndicatorRange>,
}

impl CindRanges {
    pub fn parse(line: &str) -> Option<Self> {
        let a = body(line, "+CIND")?;
        let mut indicators = Vec::new();
        for (i, g) in a.iter().enumerate() {
            let g = g.group()?;
            if g.is_empty() {
                continue; // trailing `()` some phones send
            }
            indicators.push(IndicatorRange {
                index: i as u32 + 1,
                name: g.first()?.str()?.to_string(),
                values: parse_values(&g.get(1)?.text)?,
            });
        }
        (!indicators.is_empty()).then_some(CindRanges { indicators })
    }
}

// ---------------------------------------------------------------------------------------------
// SMS (27.005)

/// `+CMTI: <mem>,<index>` / `+CDSI: <mem>,<index>` (§3.4.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageIndication {
    pub mem: String,
    pub index: u32,
}

impl MessageIndication {
    fn parse_with(line: &str, prefix: &str) -> Option<Self> {
        let a = body(line, prefix)?;
        Some(MessageIndication {
            mem: a.first()?.text()?.to_string(),
            index: int(&a, 1)?,
        })
    }
    pub fn parse_cmti(line: &str) -> Option<Self> {
        Self::parse_with(line, "+CMTI")
    }
    pub fn parse_cdsi(line: &str) -> Option<Self> {
        Self::parse_with(line, "+CDSI")
    }
}

/// `<stat>` of stored messages; numeric in PDU mode, a string in text mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MessageStatus {
    /// 0 / "REC UNREAD"
    ReceivedUnread,
    /// 1 / "REC READ"
    ReceivedRead,
    /// 2 / "STO UNSENT"
    StoredUnsent,
    /// 3 / "STO SENT"
    StoredSent,
    /// 4 / "ALL"
    All,
}

impl MessageStatus {
    fn from_arg(a: &Arg) -> Option<Self> {
        use MessageStatus::*;
        if let Some(v) = a.int::<u8>() {
            return [ReceivedUnread, ReceivedRead, StoredUnsent, StoredSent, All]
                .get(v as usize)
                .copied();
        }
        match a.text.to_ascii_uppercase().as_str() {
            "REC UNREAD" => Some(ReceivedUnread),
            "REC READ" => Some(ReceivedRead),
            "STO UNSENT" => Some(StoredUnsent),
            "STO SENT" => Some(StoredSent),
            "ALL" => Some(All),
            _ => None,
        }
    }
}

/// SMS mode a header was written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SmsMode {
    Pdu,
    Text,
}

/// `+CMGL` (§3.4.2) / `+CMGR` (§3.4.3) header; the body is the next line.
///
/// PDU mode: `+CMGL: <index>,<stat>,[<alpha>],<length>`, `+CMGR: <stat>,[<alpha>],<length>`.
/// Text mode: `+CMGL: <index>,<stat>,<oa/da>,[<alpha>],[<scts>][,<tooa/toda>,<length>]`,
/// `+CMGR: <stat>,<oa/da>,[<alpha>][,<scts>][,<tooa/toda>,<fo>,<pid>,<dcs>,<sca>,<tosca>,<length>]`.
/// Text-mode status reports and commands are not decoded field by field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageHeader {
    pub mode: SmsMode,
    /// +CMGL only.
    pub index: Option<u32>,
    pub stat: MessageStatus,
    pub alpha: Option<String>,
    /// PDU mode: TPDU octets (SMSC address excluded). Text mode: text length, when given.
    pub length: Option<u32>,
    /// Text mode `<oa>` / `<da>`.
    pub address: Option<String>,
    /// Text mode `<tooa>` / `<toda>`.
    pub address_type: Option<u8>,
    /// Text mode `<scts>` verbatim (`yy/MM/dd,hh:mm:ss±zz`).
    pub scts: Option<String>,
}

impl MessageHeader {
    pub fn parse_cmgl(line: &str) -> Option<Self> {
        let a = body(line, "+CMGL")?;
        let index = int(&a, 0)?;
        let mut h = Self::parse_rest(&a[1..], true)?;
        h.index = Some(index);
        Some(h)
    }

    pub fn parse_cmgr(line: &str) -> Option<Self> {
        let a = body(line, "+CMGR")?;
        Self::parse_rest(&a, false)
    }

    fn parse_rest(a: &[Arg], list: bool) -> Option<Self> {
        let st = a.first()?;
        let stat = MessageStatus::from_arg(st)?;
        if !st.quoted {
            // PDU mode: <stat>,[<alpha>],<length>
            return Some(MessageHeader {
                mode: SmsMode::Pdu,
                index: None,
                stat,
                alpha: text(a, 1),
                length: Some(int(a, 2)?),
                address: None,
                address_type: None,
                scts: None,
            });
        }
        // A status report (`<stat>,<fo>,<mr>,…`) or command has an unquoted number
        // where a message has its address: not decoded.
        if a.get(1).is_some_and(|x| !x.quoted && !x.text.is_empty()) {
            return None;
        }
        // Text mode (27.005 §3.4.2/§3.4.3): CMGL <oa/da>,[<alpha>],[<scts>][,<tooa/toda>,<length>];
        // CMGR DELIVER <oa>,[<alpha>],<scts>[,<tooa>,<fo>,<pid>,<dcs>,<sca>,<tosca>,<length>];
        // CMGR SUBMIT <da>,[<alpha>][,<toda>,<fo>,<pid>,<dcs>,[<vp>],<sca>,<tosca>,<length>].
        let submit = !list
            && matches!(
                stat,
                MessageStatus::StoredUnsent | MessageStatus::StoredSent
            );
        let (scts, type_at, len_at) = match (list, submit) {
            (true, _) => (text(a, 3), 4, 5),
            (false, true) => (None, 3, 10),
            (false, false) => (text(a, 3), 4, 10),
        };
        Some(MessageHeader {
            mode: SmsMode::Text,
            index: None,
            stat,
            address: text(a, 1),
            alpha: text(a, 2),
            scts,
            address_type: int(a, type_at),
            length: int(a, len_at),
        })
    }
}

/// `+CMT` header (§3.4.1). PDU: `+CMT: [<alpha>],<length>`; text:
/// `+CMT: <oa>,[<alpha>],<scts>[,<tooa>,<fo>,<pid>,<dcs>,<sca>,<tosca>,<length>]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cmt {
    pub mode: SmsMode,
    pub alpha: Option<String>,
    pub length: Option<u32>,
    pub address: Option<String>,
    pub address_type: Option<u8>,
    pub scts: Option<String>,
}

impl Cmt {
    pub fn parse(line: &str) -> Option<Self> {
        let a = body(line, "+CMT")?;
        if a.len() == 2 && !a[1].quoted {
            return Some(Cmt {
                mode: SmsMode::Pdu,
                alpha: text(&a, 0),
                length: Some(int(&a, 1)?),
                address: None,
                address_type: None,
                scts: None,
            });
        }
        if a.len() < 3 {
            return None;
        }
        Some(Cmt {
            mode: SmsMode::Text,
            address: text(&a, 0),
            alpha: text(&a, 1),
            scts: text(&a, 2),
            address_type: int(&a, 3),
            length: int(&a, 9),
        })
    }
}

/// `+CDS` (§3.4.1). PDU: `+CDS: <length>`; text: `+CDS: <fo>,<mr>,[<ra>],[<tora>],<scts>,<dt>,<st>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cds {
    pub mode: SmsMode,
    pub length: Option<u32>,
    pub fo: Option<u8>,
    pub mr: Option<u8>,
    pub ra: Option<String>,
    pub tora: Option<u8>,
    pub scts: Option<String>,
    pub dt: Option<String>,
    pub st: Option<u8>,
}

impl Cds {
    pub fn parse(line: &str) -> Option<Self> {
        let a = body(line, "+CDS")?;
        if a.len() == 1 {
            return Some(Cds {
                mode: SmsMode::Pdu,
                length: Some(int(&a, 0)?),
                fo: None,
                mr: None,
                ra: None,
                tora: None,
                scts: None,
                dt: None,
                st: None,
            });
        }
        // Text mode needs all 7 fields: `st` (index 6) is required below.
        Some(Cds {
            mode: SmsMode::Text,
            length: None,
            fo: Some(int(&a, 0)?),
            mr: Some(int(&a, 1)?),
            ra: text(&a, 2),
            tora: int(&a, 3),
            scts: text(&a, 4),
            dt: text(&a, 5),
            st: Some(int(&a, 6)?),
        })
    }
}

/// `+CMGS: <mr>[,<scts>|<ackpdu>]` (§3.5.1) / `+CMSS: <mr>[,...]` (§3.5.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageReference {
    pub mr: u8,
    /// `<scts>` (text mode) or `<ackpdu>` (PDU mode), verbatim.
    pub ack: Option<String>,
}

impl MessageReference {
    fn parse_with(line: &str, prefix: &str) -> Option<Self> {
        let a = body(line, prefix)?;
        Some(MessageReference {
            mr: int(&a, 0)?,
            ack: text(&a, 1),
        })
    }
    pub fn parse_cmgs(line: &str) -> Option<Self> {
        Self::parse_with(line, "+CMGS")
    }
    pub fn parse_cmss(line: &str) -> Option<Self> {
        Self::parse_with(line, "+CMSS")
    }
}

/// One storage of `+CPMS?`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageUsage {
    /// `None` in the set-command response, which has counts only.
    pub mem: Option<String>,
    pub used: u32,
    pub total: u32,
}

/// `+CPMS: <mem1>,<used1>,<total1>,<mem2>,...` (read) or `+CPMS: <used1>,<total1>,...` (set).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CpmsStatus {
    pub storages: Vec<StorageUsage>,
}

impl CpmsStatus {
    pub fn parse(line: &str) -> Option<Self> {
        let a = body(line, "+CPMS")?;
        let with_mem = a.first()?.int::<u32>().is_none();
        let step = if with_mem { 3 } else { 2 };
        if a.is_empty() || a.len() % step != 0 {
            return None;
        }
        let storages = a
            .chunks(step)
            .map(|c| {
                let o = step - 2;
                Some(StorageUsage {
                    mem: if with_mem {
                        Some(c[0].text()?.to_string())
                    } else {
                        None
                    },
                    used: c[o].int()?,
                    total: c[o + 1].int()?,
                })
            })
            .collect::<Option<_>>()?;
        Some(CpmsStatus { storages })
    }
}

/// `+CPMS: (list of <mem1>s),(list of <mem2>s),(list of <mem3>s)` test-command response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CpmsSupport {
    pub mem1: Vec<String>,
    pub mem2: Vec<String>,
    pub mem3: Vec<String>,
}

impl CpmsSupport {
    pub fn parse(line: &str) -> Option<Self> {
        let a = body(line, "+CPMS")?;
        let list = |i: usize| -> Vec<String> {
            let Some(g) = a.get(i) else {
                return Vec::new();
            };
            // A single storage may come without parentheses.
            let items = g.group().unwrap_or_else(|| vec![g.clone()]);
            items
                .iter()
                .filter_map(|m| m.text().map(str::to_string))
                .collect()
        };
        if !a.first()?.text.starts_with('(') {
            return None;
        }
        Some(CpmsSupport {
            mem1: list(0),
            mem2: list(1),
            mem3: list(2),
        })
    }
}

/// `+CNMI: <mode>,<mt>,<bm>,<ds>,<bfr>` (§3.4.1, read).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cnmi {
    pub mode: u8,
    pub mt: Option<u8>,
    pub bm: Option<u8>,
    pub ds: Option<u8>,
    pub bfr: Option<u8>,
}

impl Cnmi {
    pub fn parse(line: &str) -> Option<Self> {
        let a = body(line, "+CNMI")?;
        Some(Cnmi {
            mode: int(&a, 0)?,
            mt: int(&a, 1),
            bm: int(&a, 2),
            ds: int(&a, 3),
            bfr: int(&a, 4),
        })
    }
}

/// `+CNMI: (list of <mode>s),(list of <mt>s),...` test-command response, ranges expanded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CnmiSupport {
    pub mode: Vec<u32>,
    pub mt: Vec<u32>,
    pub bm: Vec<u32>,
    pub ds: Vec<u32>,
    pub bfr: Vec<u32>,
}

impl CnmiSupport {
    pub fn parse(line: &str) -> Option<Self> {
        let a = body(line, "+CNMI")?;
        let get = |i: usize| match a.get(i) {
            Some(g) if g.text.starts_with('(') => parse_values(&g.text),
            Some(_) => None,
            None => Some(Vec::new()),
        };
        Some(CnmiSupport {
            mode: get(0)?,
            mt: get(1)?,
            bm: get(2)?,
            ds: get(3)?,
            bfr: get(4)?,
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Operator selection: +COPS (§7.3)

/// `+COPS: <mode>[,<format>,<oper>[,<AcT>]]` read-command response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cops {
    /// 0 automatic, 1 manual, 2 deregister, 3 set format only, 4 manual/automatic.
    pub mode: u8,
    /// 0 long alphanumeric, 1 short alphanumeric, 2 numeric.
    pub format: Option<u8>,
    pub oper: Option<String>,
    pub act: Option<AccessTech>,
}

impl Cops {
    pub fn parse(line: &str) -> Option<Self> {
        let a = body(line, "+COPS")?;
        if a.first()?.text.starts_with('(') {
            return None; // test-command response
        }
        Some(Cops {
            mode: int(&a, 0)?,
            format: int(&a, 1),
            oper: text(&a, 2),
            act: AccessTech::from_arg(&a, 3),
        })
    }
}

/// `<stat>` of a +COPS=? entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OperatorStatus {
    Unknown,
    Available,
    Current,
    Forbidden,
    Other(u8),
}

/// One `(<stat>,long,short,numeric[,<AcT>])` entry of +COPS=?.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorEntry {
    pub stat: OperatorStatus,
    pub long: Option<String>,
    pub short: Option<String>,
    pub numeric: Option<String>,
    pub act: Option<AccessTech>,
}

/// `+COPS: [list of (<stat>,long,short,numeric[,<AcT>])s][,,(list of <mode>s),(list of <format>s)]`.
/// The trailing mode/format lists are skipped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CopsList {
    pub operators: Vec<OperatorEntry>,
}

impl CopsList {
    pub fn parse(line: &str) -> Option<Self> {
        let a = body(line, "+COPS")?;
        if !a.first()?.text.starts_with('(') {
            return None;
        }
        let mut operators: Vec<OperatorEntry> = Vec::new();
        for f in &a {
            // Fields outside a tuple (the empty `,,` separator) carry no operator.
            let Some(g) = f.group() else {
                continue;
            };
            // Mode / format lists have no quoted names.
            if g.len() < 4 || !g[1..4].iter().any(|x| x.quoted) {
                continue;
            }
            let stat = match g[0].int::<u8>() {
                Some(0) => OperatorStatus::Unknown,
                Some(1) => OperatorStatus::Available,
                Some(2) => OperatorStatus::Current,
                Some(3) => OperatorStatus::Forbidden,
                Some(v) => OperatorStatus::Other(v),
                None => continue,
            };
            let act = AccessTech::from_arg(&g, 4);
            operators.push(OperatorEntry {
                stat,
                long: text(&g, 1).filter(|s| !s.is_empty()),
                short: text(&g, 2).filter(|s| !s.is_empty()),
                numeric: text(&g, 3).filter(|s| !s.is_empty()),
                act,
            });
        }
        Some(CopsList { operators })
    }
}

// ---------------------------------------------------------------------------------------------
// Packet domain events: +CGEV (§10.1.19)

/// A `+CGEV` event. Old-style address forms (`NW DEACT <PDP_type>,<PDP_addr>[,<cid>]`) and
/// the current cid forms are both decoded; anything else is [`CgevEvent::Other`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "event",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum CgevEvent {
    /// `REJECT <PDP_type>,<PDP_addr>`
    Reject { pdp_type: String, pdp_addr: String },
    /// `NW REACT <PDP_type>,<PDP_addr>[,<cid>]`
    NwReact {
        pdp_type: String,
        pdp_addr: String,
        cid: Option<u32>,
    },
    /// `NW DEACT <PDP_type>,<PDP_addr>[,<cid>]`
    NwDeactPdp {
        pdp_type: String,
        pdp_addr: String,
        cid: Option<u32>,
    },
    /// `ME DEACT <PDP_type>,<PDP_addr>[,<cid>]`
    MeDeactPdp {
        pdp_type: String,
        pdp_addr: String,
        cid: Option<u32>,
    },
    /// `NW PDN ACT <cid>[,<WLAN_Offload>]`
    NwPdnAct { cid: u32 },
    /// `ME PDN ACT <cid>[,<reason>[,<cid_other>]]`
    MePdnAct {
        cid: u32,
        reason: Option<u32>,
        cid_other: Option<u32>,
    },
    /// `NW PDN DEACT <cid>[,<WLAN_Offload>]`
    NwPdnDeact { cid: u32 },
    /// `ME PDN DEACT <cid>`
    MePdnDeact { cid: u32 },
    /// `NW ACT <p_cid>,<cid>,<event_type>[,<WLAN_Offload>]`
    NwAct {
        p_cid: u32,
        cid: u32,
        event_type: u32,
    },
    /// `ME ACT <p_cid>,<cid>,<event_type>[,<WLAN_Offload>]`
    MeAct {
        p_cid: u32,
        cid: u32,
        event_type: u32,
    },
    /// `NW DEACT <p_cid>,<cid>,<event_type>[,<WLAN_Offload>]`
    NwDeact {
        p_cid: u32,
        cid: u32,
        event_type: u32,
    },
    /// `ME DEACT <p_cid>,<cid>,<event_type>`
    MeDeact {
        p_cid: u32,
        cid: u32,
        event_type: u32,
    },
    /// `NW MODIFY <cid>,<change_reason>,<event_type>[,<WLAN_Offload>]`
    NwModify {
        cid: u32,
        change_reason: u32,
        event_type: u32,
    },
    /// `ME MODIFY <cid>,<change_reason>,<event_type>[,<WLAN_Offload>]`
    MeModify {
        cid: u32,
        change_reason: u32,
        event_type: u32,
    },
    /// `NW DETACH`
    NwDetach,
    /// `ME DETACH`
    MeDetach,
    /// `NW CLASS <class>`
    NwClass { class: String },
    /// `ME CLASS <class>`
    MeClass { class: String },
    /// Anything else (vendor or newer events).
    Other,
}

/// `+CGEV: <event>`: the decoded event plus the text after `+CGEV:`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cgev {
    pub event: CgevEvent,
    pub raw: String,
}

impl Cgev {
    pub fn parse(line: &str) -> Option<Self> {
        Some(Self::parse_event(strip_prefix(line, "+CGEV")?))
    }

    /// Decodes the text after `+CGEV:` (e.g. `NW PDN ACT 1`).
    pub fn parse_event(text: &str) -> Self {
        let raw = text.trim().to_string();
        Cgev {
            event: Self::decode(&raw).unwrap_or(CgevEvent::Other),
            raw,
        }
    }

    fn decode(raw: &str) -> Option<CgevEvent> {
        use CgevEvent::*;
        let upper = raw.to_ascii_uppercase();
        // Keywords first; the argument list starts at the first token that is not a keyword.
        const KEYS: [&str; 10] = [
            "NW", "ME", "PDN", "ACT", "DEACT", "REACT", "MODIFY", "DETACH", "CLASS", "REJECT",
        ];
        let mut cut = 0;
        let mut words = Vec::new();
        for w in upper.split_whitespace() {
            if !KEYS.contains(&w) {
                break;
            }
            words.push(w);
            cut = upper[cut..].find(w).map(|p| cut + p + w.len())?;
        }
        let a = split(&raw[cut..]);
        let n = |i: usize| int::<u32>(&a, i);
        let s = |i: usize| text(&a, i);
        let addr = || Some((s(0)?, s(1)?));
        // Old-style forms have a non-numeric <PDP_type> first.
        let addr_form = a
            .first()
            .is_some_and(|f| !f.is_empty() && f.int::<u32>().is_none());
        Some(match words.as_slice() {
            ["REJECT"] => {
                let (pdp_type, pdp_addr) = addr()?;
                Reject { pdp_type, pdp_addr }
            }
            ["NW", "REACT"] => {
                let (pdp_type, pdp_addr) = addr()?;
                NwReact {
                    pdp_type,
                    pdp_addr,
                    cid: n(2),
                }
            }
            ["NW", "DEACT"] if addr_form => {
                let (pdp_type, pdp_addr) = addr()?;
                NwDeactPdp {
                    pdp_type,
                    pdp_addr,
                    cid: n(2),
                }
            }
            ["ME", "DEACT"] if addr_form => {
                let (pdp_type, pdp_addr) = addr()?;
                MeDeactPdp {
                    pdp_type,
                    pdp_addr,
                    cid: n(2),
                }
            }
            ["NW", "PDN", "ACT"] => NwPdnAct { cid: n(0)? },
            ["ME", "PDN", "ACT"] => MePdnAct {
                cid: n(0)?,
                reason: n(1),
                cid_other: n(2),
            },
            ["NW", "PDN", "DEACT"] => NwPdnDeact { cid: n(0)? },
            ["ME", "PDN", "DEACT"] => MePdnDeact { cid: n(0)? },
            ["NW", "ACT"] => NwAct {
                p_cid: n(0)?,
                cid: n(1)?,
                event_type: n(2)?,
            },
            ["ME", "ACT"] => MeAct {
                p_cid: n(0)?,
                cid: n(1)?,
                event_type: n(2)?,
            },
            ["NW", "DEACT"] => NwDeact {
                p_cid: n(0)?,
                cid: n(1)?,
                event_type: n(2)?,
            },
            ["ME", "DEACT"] => MeDeact {
                p_cid: n(0)?,
                cid: n(1)?,
                event_type: n(2)?,
            },
            ["NW", "MODIFY"] => NwModify {
                cid: n(0)?,
                change_reason: n(1)?,
                event_type: n(2)?,
            },
            ["ME", "MODIFY"] => MeModify {
                cid: n(0)?,
                change_reason: n(1)?,
                event_type: n(2)?,
            },
            ["NW", "DETACH"] => NwDetach,
            ["ME", "DETACH"] => MeDetach,
            ["NW", "CLASS"] => NwClass { class: s(0)? },
            ["ME", "CLASS"] => MeClass { class: s(0)? },
            _ => return None,
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Dispatcher

/// An unsolicited result code recognised by [`parse_any`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "urc", content = "data", rename_all = "camelCase")]
pub enum Urc {
    /// +CREG / +CGREG / +CEREG / +C5GREG (either form).
    Registration(Registration),
    Clip(CallerId),
    Colp(CallerId),
    Ccwa(CallWaiting),
    Ciev(Ciev),
    Cmti(MessageIndication),
    Cdsi(MessageIndication),
    /// Header; the PDU / text is the next line.
    Cmt(Cmt),
    /// Header in PDU mode (PDU on the next line); the whole report in text mode.
    Cds(Cds),
    Cgev(Cgev),
}

/// Parses one line as any supported URC. `+CUSD` is handled by the USSD module.
pub fn parse_any(line: &str) -> Option<Urc> {
    let head = line.trim_start().get(..3)?.to_ascii_uppercase();
    match head.as_str() {
        "+CR" | "+CG" | "+CE" | "+C5" => Registration::parse(line)
            .map(Urc::Registration)
            .or_else(|| Cgev::parse(line).map(Urc::Cgev)),
        _ => CallerId::parse_clip(line)
            .map(Urc::Clip)
            .or_else(|| CallerId::parse_colp(line).map(Urc::Colp))
            .or_else(|| CallWaiting::parse(line).map(Urc::Ccwa))
            .or_else(|| Ciev::parse(line).map(Urc::Ciev))
            .or_else(|| MessageIndication::parse_cmti(line).map(Urc::Cmti))
            .or_else(|| MessageIndication::parse_cdsi(line).map(Urc::Cdsi))
            .or_else(|| Cmt::parse(line).map(Urc::Cmt))
            .or_else(|| Cds::parse(line).map(Urc::Cds)),
    }
}
