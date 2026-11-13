//! Properties of the public setting enums and log levels: numbers, serialport
//! conversions, serde round trips, and which messages a level logs.

use proptest::prelude::*;
use proptest::test_runner::Config;
use serialplugin_core::state::{DataBits, FlowControl, LogLevel, Parity, StopBits};

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
    }
}
