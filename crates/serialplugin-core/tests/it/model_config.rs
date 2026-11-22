//! Model tests of everything that is configuration: setting enums and their numbers, log
//! levels, AT session and exchange defaults, `AT+CMUX` parameters, write timeouts, event
//! JSON. Each value in is checked against the rule written out here.

use crate::common::FakeDevice;
use proptest::collection::vec;
use proptest::option;
use proptest::prelude::*;
use proptest::test_runner::Config;
use serde_json::{json, Value};
use serialplugin_core::at::parse::AtParseStatus;
use serialplugin_core::at::session::{
    check_expect_ok, normalize_at_command, AtCommandOptions, AtPhase, AtPhaseWrite, AtSessionConfig,
};
use serialplugin_core::cmux::{
    cmux_n1, mux_path, parse_mux_path, CmuxPhysicalIo, CmuxTimers, SerialPortIo,
};
use serialplugin_core::events::{
    AtResultFormat, ExchangeCompletionMode, ExchangeOptions, RxPrepareMode, SerialEvent,
};
use serialplugin_core::state::{
    get_log_level, set_log_level, DataBits, FlowControl, LogLevel, Parity, StopBits,
};
use serialport::SerialPort;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const DATA_BITS: [DataBits; 4] = [
    DataBits::Five,
    DataBits::Six,
    DataBits::Seven,
    DataBits::Eight,
];
const FLOW: [FlowControl; 3] = [
    FlowControl::None,
    FlowControl::Software,
    FlowControl::Hardware,
];
const PARITY: [Parity; 3] = [Parity::None, Parity::Odd, Parity::Even];
const STOP_BITS: [StopBits; 2] = [StopBits::One, StopBits::Two];
/// Least to most verbose.
const LEVELS: [LogLevel; 5] = [
    LogLevel::None,
    LogLevel::Error,
    LogLevel::Warn,
    LogLevel::Info,
    LogLevel::Debug,
];

fn pick<T: Copy + std::fmt::Debug + 'static>(all: &'static [T]) -> impl Strategy<Value = T> {
    prop::sample::select(all)
}

/// Two values are equal exactly when their numbers are; serde keeps the value.
fn same_iff_same_number<T>(a: T, b: T, num: impl Fn(&T) -> u8) -> Result<(), TestCaseError>
where
    T: PartialEq + serde::Serialize + serde::de::DeserializeOwned + std::fmt::Debug,
{
    prop_assert_eq!(a == b, num(&a) == num(&b));
    let back: T = serde_json::from_value(serde_json::to_value(&a).unwrap()).unwrap();
    prop_assert_eq!(back, a);
    Ok(())
}

proptest! {
    #![proptest_config(Config { cases: 64, failure_persistence: None, ..Config::default() })]

    /// Data and stop bits are their count, the same as serialport's number for them.
    #[test]
    fn data_and_stop_bits_are_their_count(
        a in pick(&DATA_BITS), b in pick(&DATA_BITS), s in pick(&STOP_BITS), t in pick(&STOP_BITS)
    ) {
        prop_assert!((5..=8).contains(&a.as_u8()));
        prop_assert_eq!(u8::from(serialport::DataBits::from(a)), a.as_u8());
        prop_assert_eq!(serialport::DataBits::try_from(a.as_u8()), Ok(serialport::DataBits::from(a)));
        same_iff_same_number(a, b, DataBits::as_u8)?;
        prop_assert_eq!(u8::from(serialport::StopBits::from(s)), s.as_u8());
        prop_assert_eq!(serialport::StopBits::try_from(s.as_u8()), Ok(serialport::StopBits::from(s)));
        same_iff_same_number(s, t, StopBits::as_u8)?;
    }

    /// Flow control and parity number their variants 0, 1, 2 in declaration order and
    /// map onto the serialport variant of the same name.
    #[test]
    fn flow_and_parity_number_their_variants(
        f in pick(&FLOW), g in pick(&FLOW), p in pick(&PARITY), q in pick(&PARITY)
    ) {
        prop_assert_eq!(FLOW[f.as_u8() as usize], f);
        prop_assert_eq!(format!("{:?}", serialport::FlowControl::from(f)), format!("{f:?}"));
        same_iff_same_number(f, g, FlowControl::as_u8)?;
        prop_assert_eq!(PARITY[p.as_u8() as usize], p);
        prop_assert_eq!(format!("{:?}", serialport::Parity::from(p)), format!("{p:?}"));
        same_iff_same_number(p, q, Parity::as_u8)?;
    }

    /// A level logs a kind of message exactly when it is at least that verbose; the
    /// default is Info.
    #[test]
    fn a_level_logs_what_is_at_least_as_severe(level in pick(&LEVELS)) {
        let rank = |l: LogLevel| LEVELS.iter().position(|x| *x == l).unwrap();
        let r = rank(level);
        prop_assert_eq!(level.should_log_error(), r >= rank(LogLevel::Error));
        prop_assert_eq!(level.should_log_warn(), r >= rank(LogLevel::Warn));
        prop_assert_eq!(level.should_log_info(), r >= rank(LogLevel::Info));
        prop_assert_eq!(level.should_log_debug(), r >= rank(LogLevel::Debug));
        prop_assert_eq!(LogLevel::default(), LogLevel::Info);
        // The level is process-wide; logging works at every one.
        set_log_level(level);
        prop_assert_eq!(get_log_level(), level);
        serialplugin_core::log_error!("error at {:?}", level);
        serialplugin_core::log_warn!("warn at {:?}", level);
        serialplugin_core::log_info!("info at {:?}", level);
        serialplugin_core::log_debug!("debug at {:?}", level);
        set_log_level(LogLevel::default());
    }

    /// An AT command takes the per-command value, else the session's, else the default.
    #[test]
    fn at_options_fall_back_to_the_session((s, per) in (session(), option::of(command_options()))) {
        let merged = s.merge_exchange("AT+X", per.as_ref());
        let per = per.unwrap_or_else(no_command_options);
        prop_assert_eq!(json_of(&merged), json_of(&ExchangeOptions {
            timeout_ms: per.timeout_ms.or(s.default_timeout_ms),
            terminators: per.terminators.or(s.default_terminators.clone()),
            idle_ms: per.idle_ms.or(s.default_idle_ms),
            rx_prepare: per.rx_prepare.or(s.default_rx_prepare.clone()).or(Some(RxPrepareMode::Drain)),
            completion_mode: per.completion_mode.or(Some(ExchangeCompletionMode::AtFinalLine)),
            result_format: per.result_format.or(s.result_format),
            command: Some("AT+X".into()),
            solicited_prefixes: per.solicited_prefixes.or(s.default_solicited_prefixes.clone()),
            ..Default::default()
        }));
        prop_assert_eq!(s.stop_on_error(), s.stop_on_error.unwrap_or(true));
        prop_assert_eq!(s.expect_ok(), s.expect_ok.unwrap_or(false));
        prop_assert_eq!(s.append_cr(), s.append_cr.unwrap_or(true));
        for status in [AtParseStatus::Ok, AtParseStatus::Error, AtParseStatus::Cme] {
            let fails = s.expect_ok == Some(true) && status != AtParseStatus::Ok;
            prop_assert_eq!(check_expect_ok(&s, status, "reply").is_err(), fails);
        }
    }

    /// So does a phase of `at_phases`.
    #[test]
    fn phases_fall_back_to_the_session((s, per) in (session(), command_options())) {
        let phase = AtPhase {
            write: AtPhaseWrite::Text("AT+P".into()),
            completion_mode: per.completion_mode.clone(),
            result_format: per.result_format,
            timeout_ms: per.timeout_ms,
            command: None,
            rx_prepare: per.rx_prepare.clone(),
        };
        prop_assert_eq!(json_of(&s.merge_phase(&phase, "AT+P")), json_of(&ExchangeOptions {
            timeout_ms: per.timeout_ms.or(s.default_timeout_ms),
            terminators: s.default_terminators.clone(),
            idle_ms: s.default_idle_ms,
            rx_prepare: per.rx_prepare.or(s.default_rx_prepare.clone()).or(Some(RxPrepareMode::Drain)),
            completion_mode: per.completion_mode.or(Some(ExchangeCompletionMode::AtFinalLine)),
            result_format: per.result_format.or(s.result_format),
            command: Some("AT+P".into()),
            solicited_prefixes: s.default_solicited_prefixes.clone(),
            ..Default::default()
        }));
    }

    /// A command gets one CR unless it ends in CR or LF already, or CR is turned off.
    #[test]
    fn a_command_ends_in_one_cr(
        body in "AT[+A-Z0-9?=]{0,8}", end in prop::sample::select(vec!["", "\r", "\r\n", "\n"]), cr in any::<bool>()
    ) {
        let command = format!("{body}{end}");
        let want = if cr && end.is_empty() { format!("{body}\r") } else { command.clone() };
        prop_assert_eq!(normalize_at_command(&command, cr), want);
    }

    /// Exchange options resolve to what was given, else to the defaults.
    #[test]
    fn exchange_options_resolve_to_the_defaults(
        timeout in option::of(any::<u64>()), max in option::of(any::<usize>()),
        prepare in option::of(prepare()), mode in option::of(mode()), format in option::of(format()),
        drain in option::of((any::<u64>(), any::<u64>())), terminators in option::of(vec("[A-Z]{1,4}", 0..3)),
    ) {
        let r = ExchangeOptions {
            timeout_ms: timeout,
            max_bytes: max,
            rx_prepare: prepare.clone(),
            completion_mode: mode.clone(),
            result_format: format,
            drain_idle_ms: drain.map(|d| d.0),
            drain_max_ms: drain.map(|d| d.1),
            terminators: terminators.clone(),
            ..Default::default()
        }
        .resolve();
        prop_assert_eq!(r.timeout_ms, timeout.unwrap_or(5000));
        prop_assert_eq!(r.max_bytes, max.unwrap_or(4096));
        prop_assert_eq!(r.rx_prepare, prepare.unwrap_or(RxPrepareMode::Drain));
        prop_assert_eq!(r.completion_mode, mode.unwrap_or(ExchangeCompletionMode::AtFinalLine));
        prop_assert_eq!(r.result_format, format.unwrap_or(AtResultFormat::Verbose));
        prop_assert_eq!((r.drain_idle_ms, r.drain_max_ms), drain.unwrap_or((50, 200)));
        let terminators = terminators.unwrap_or_else(|| {
            ["\r\nOK\r\n", "\r\nERROR\r\n", "\nOK\n", "\nERROR\n"].map(String::from).to_vec()
        });
        prop_assert_eq!(r.terminators, terminators.into_iter().map(String::into_bytes).collect::<Vec<_>>());
    }

    /// `AT+CMUX=mode,subset,speed,N1,T1,N2,T2,…`: N1 as given (31 by default), the timers in
    /// 10 ms units inside the 27.007 ranges, else the defaults; without T1 and N2 SABM is
    /// sent once.
    #[test]
    fn cmux_parameters_come_from_the_command(
        params in vec(option::of(0u64..300), 0..9), upper in any::<bool>(), spaced in any::<bool>()
    ) {
        let shown: Vec<String> = params.iter().map(|p| match p {
            Some(v) if spaced => format!(" {v} "),
            Some(v) => v.to_string(),
            None => String::new(),
        }).collect();
        let name = if upper { "AT+CMUX" } else { "at+cmux" };
        let command = format!("{name}={}", shown.join(","));
        let at = |i: usize| params.get(i).copied().flatten();
        prop_assert_eq!(cmux_n1(&command), at(3).map_or(31, |n| n as usize));
        let tens = |v: Option<u64>, min: u64, default: u64| {
            Duration::from_millis(v.filter(|v| (min..=255).contains(v)).map_or(default, |v| v * 10))
        };
        let n2 = match (at(4), at(5)) {
            (None, None) => 0,
            (_, Some(n)) if n <= 100 => n as u32,
            _ => 3,
        };
        let want = CmuxTimers { t1: tens(at(4), 1, 100), n2, t2: tens(at(6), 2, 300) };
        prop_assert_eq!(CmuxTimers::from_command(&command), want);
        prop_assert_eq!(cmux_n1("AT"), 31);
        let defaults = CmuxTimers { n2: 0, ..CmuxTimers::default() };
        prop_assert_eq!(CmuxTimers::from_command("AT"), defaults);
        prop_assert_eq!(CmuxTimers::default().n2, 3);
    }

    /// A channel path names its port and DLCI; only DLCI 1..=63 carry data.
    #[test]
    fn mux_paths_parse_back(base in "/dev/[a-z0-9#=]{1,12}", dlci in any::<u8>()) {
        let want = (1..=63).contains(&dlci).then_some((base.as_str(), dlci));
        let path = mux_path(&base, dlci);
        prop_assert_eq!(parse_mux_path(&path), want);
        prop_assert_eq!(parse_mux_path("/dev/ttyUSB0"), None);
    }

    /// A mux write may take twice its wire time (start, data, parity and stop bits) plus a
    /// second; the port's own timeout comes back after it.
    #[test]
    fn mux_write_timeout_is_twice_the_wire_time_plus_a_second(
        baud in any::<u32>(), bits in pick(&DATA_BITS), parity in pick(&PARITY), stop in pick(&STOP_BITS),
        len in 1usize..4000,
    ) {
        let device = FakeDevice::new();
        let mut port = device.port();
        port.set_baud_rate(baud).unwrap();
        port.set_data_bits(bits.into()).unwrap();
        port.set_parity(parity.into()).unwrap();
        port.set_stop_bits(stop.into()).unwrap();
        let shared: Arc<Mutex<Box<dyn SerialPort>>> = Arc::new(Mutex::new(Box::new(port)));
        let io = SerialPortIo(shared.clone());
        let per_char = 1 + u64::from(bits.as_u8()) + u64::from(parity != Parity::None) + u64::from(stop.as_u8());
        let wire = len as u64 * per_char * 1000 / u64::from(baud.max(1));
        let want = Duration::from_millis(2 * wire + 1000);
        prop_assert_eq!(io.write_timeout(len), want);
        io.write_all(&vec![7; len]).unwrap();
        prop_assert_eq!(device.write_timeouts(), vec![want]);
        prop_assert_eq!(device.written(), vec![7; len]);
        prop_assert_eq!(shared.lock().unwrap().timeout(), Duration::from_secs(1));

        struct Plain;
        impl CmuxPhysicalIo for Plain {
            fn write_all(&self, _: &[u8]) -> Result<(), String> {
                Ok(())
            }
        }
        prop_assert_eq!(Plain.write_timeout(len), Duration::from_secs(1));
    }

    /// Watch events keep their 4.x JSON form, both ways.
    #[test]
    fn events_keep_their_json_form(path in "\\PC{0,12}", text in "\\PC{0,12}", bytes in vec(any::<u8>(), 0..16)) {
        let n = bytes.len();
        for (event, json) in [
            (SerialEvent::Data { path: path.clone(), data: bytes.clone(), size: n },
             json!({"kind": "data", "path": &path, "data": &bytes, "size": n})),
            (SerialEvent::Urc { path: path.clone(), line: text.clone() },
             json!({"kind": "urc", "path": &path, "line": &text})),
            (SerialEvent::Error { path: path.clone(), message: text.clone() },
             json!({"kind": "error", "path": &path, "message": &text})),
            (SerialEvent::Disconnect { path: path.clone(), reason: text.clone() },
             json!({"kind": "disconnect", "path": &path, "reason": &text})),
        ] {
            prop_assert_eq!(&serde_json::to_value(&event).unwrap(), &json);
            let back: SerialEvent = serde_json::from_value(json.clone()).unwrap();
            prop_assert_eq!(serde_json::to_value(back).unwrap(), json);
        }
    }
}

fn json_of(v: &impl serde::Serialize) -> Value {
    serde_json::to_value(v).unwrap()
}

fn prepare() -> impl Strategy<Value = RxPrepareMode> {
    prop::sample::select(vec![
        RxPrepareMode::Drain,
        RxPrepareMode::Purge,
        RxPrepareMode::None,
    ])
}

fn mode() -> impl Strategy<Value = ExchangeCompletionMode> {
    prop::sample::select(vec![
        ExchangeCompletionMode::AtFinalLine,
        ExchangeCompletionMode::AtIntermediate,
        ExchangeCompletionMode::Substring,
    ])
}

fn format() -> impl Strategy<Value = AtResultFormat> {
    prop::sample::select(vec![AtResultFormat::Verbose, AtResultFormat::Numeric])
}

fn strings() -> impl Strategy<Value = Option<Vec<String>>> {
    option::of(vec("[+A-Z:]{1,4}", 0..3))
}

fn session() -> impl Strategy<Value = AtSessionConfig> {
    let flags = (
        option::of(any::<bool>()),
        option::of(any::<bool>()),
        option::of(any::<bool>()),
    );
    let times = (option::of(any::<u64>()), option::of(any::<u64>()));
    (
        times,
        strings(),
        strings(),
        option::of(prepare()),
        option::of(format()),
        flags,
    )
        .prop_map(
            |(
                (timeout, idle),
                terminators,
                prefixes,
                prepare,
                format,
                (stop, expect_ok, append_cr),
            )| AtSessionConfig {
                default_timeout_ms: timeout,
                default_terminators: terminators,
                default_idle_ms: idle,
                default_rx_prepare: prepare,
                default_solicited_prefixes: prefixes,
                stop_on_error: stop,
                expect_ok,
                append_cr,
                result_format: format,
            },
        )
}

fn no_command_options() -> AtCommandOptions {
    AtCommandOptions {
        timeout_ms: None,
        terminators: None,
        idle_ms: None,
        rx_prepare: None,
        completion_mode: None,
        result_format: None,
        solicited_prefixes: None,
        append_cr: None,
    }
}

fn command_options() -> impl Strategy<Value = AtCommandOptions> {
    (
        option::of(any::<u64>()),
        strings(),
        option::of(any::<u64>()),
        option::of(prepare()),
        option::of(mode()),
        option::of(format()),
        strings(),
    )
        .prop_map(
            |(
                timeout_ms,
                terminators,
                idle_ms,
                rx_prepare,
                completion_mode,
                result_format,
                solicited_prefixes,
            )| AtCommandOptions {
                timeout_ms,
                terminators,
                idle_ms,
                rx_prepare,
                completion_mode,
                result_format,
                solicited_prefixes,
                append_cr: None,
            },
        )
}
