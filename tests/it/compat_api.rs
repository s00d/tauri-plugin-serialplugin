//! The 4.0.0 `api::SerialPort<R>` methods are inherent, with the 4.0.0 signatures, so
//! path calls (`SerialPort::<R>::open(&s, ..)`) keep compiling. Generated from the 4.0.0
//! sources (`src/api/serial.rs`).

use std::collections::HashMap;
use std::time::Duration;
use tauri::ipc::Channel;
use tauri::Runtime;
use tauri_plugin_serialplugin::api::SerialPort;
use tauri_plugin_serialplugin::at::session::{
    AtCommandOptions, AtPhase, AtSessionConfig, SendSmsPduOptions,
};
use tauri_plugin_serialplugin::error::Error;
use tauri_plugin_serialplugin::events::{
    ExchangeOptions, PortListEvent, SerialEvent, WatchOptions, WatchPortsOptions,
};
use tauri_plugin_serialplugin::state::{ClearBuffer, DataBits, FlowControl, Parity, StopBits};

#[allow(clippy::type_complexity)]
fn assert_signatures<R: Runtime>() {
    let _: fn(&SerialPort<R>, bool) -> Result<HashMap<String, HashMap<String, String>>, Error> =
        SerialPort::<R>::available_ports;
    let _: fn(&SerialPort<R>, u32) -> Result<(), Error> = SerialPort::<R>::unwatch_ports;
    let _: fn(&SerialPort<R>) -> Result<Vec<String>, Error> = SerialPort::<R>::managed_ports;
    let _: fn(&SerialPort<R>, String) -> Result<(), Error> = SerialPort::<R>::cancel_read;
    let _: fn(&SerialPort<R>, String) -> Result<(), Error> = SerialPort::<R>::close;
    let _: fn(&SerialPort<R>) -> Result<(), Error> = SerialPort::<R>::close_all;
    let _: fn(&SerialPort<R>, String) -> Result<(), Error> = SerialPort::<R>::force_close;
    let _: fn(
        &SerialPort<R>,
        String,
        u32,
        Option<DataBits>,
        Option<FlowControl>,
        Option<Parity>,
        Option<StopBits>,
        Option<u64>,
    ) -> Result<String, Error> = SerialPort::<R>::open;
    let _: fn(
        &SerialPort<R>,
        String,
        u32,
        Option<DataBits>,
        Option<FlowControl>,
        Option<Parity>,
        Option<StopBits>,
        Option<u64>,
        tauri_plugin_serialplugin::state::ModemLinesOnOpen,
    ) -> Result<String, Error> = SerialPort::<R>::open_with_lines;
    let _: fn(&SerialPort<R>, u32) -> Result<(), Error> = SerialPort::<R>::unwatch;
    let _: fn(&SerialPort<R>, String, Option<u64>, Option<usize>) -> Result<String, Error> =
        SerialPort::<R>::read;
    let _: fn(&SerialPort<R>, String, Option<u64>, Option<usize>) -> Result<Vec<u8>, Error> =
        SerialPort::<R>::read_binary;
    let _: fn(&SerialPort<R>, String, String) -> Result<usize, Error> = SerialPort::<R>::write;
    let _: fn(&SerialPort<R>, String, Vec<u8>) -> Result<usize, Error> =
        SerialPort::<R>::write_binary;
    let _: fn(&SerialPort<R>, String, u32) -> Result<(), Error> = SerialPort::<R>::set_baud_rate;
    let _: fn(&SerialPort<R>, String, DataBits) -> Result<(), Error> =
        SerialPort::<R>::set_data_bits;
    let _: fn(&SerialPort<R>, String, FlowControl) -> Result<(), Error> =
        SerialPort::<R>::set_flow_control;
    let _: fn(&SerialPort<R>, String, Parity) -> Result<(), Error> = SerialPort::<R>::set_parity;
    let _: fn(&SerialPort<R>, String, StopBits) -> Result<(), Error> =
        SerialPort::<R>::set_stop_bits;
    let _: fn(&SerialPort<R>, String, Duration) -> Result<(), Error> = SerialPort::<R>::set_timeout;
    let _: fn(&SerialPort<R>, String, bool) -> Result<(), Error> =
        SerialPort::<R>::write_request_to_send;
    let _: fn(&SerialPort<R>, String, bool) -> Result<(), Error> =
        SerialPort::<R>::write_data_terminal_ready;
    let _: fn(&SerialPort<R>, String) -> Result<bool, Error> = SerialPort::<R>::read_clear_to_send;
    let _: fn(&SerialPort<R>, String) -> Result<bool, Error> = SerialPort::<R>::read_data_set_ready;
    let _: fn(&SerialPort<R>, String) -> Result<bool, Error> = SerialPort::<R>::read_ring_indicator;
    let _: fn(&SerialPort<R>, String) -> Result<bool, Error> = SerialPort::<R>::read_carrier_detect;
    let _: fn(&SerialPort<R>, String) -> Result<u32, Error> = SerialPort::<R>::bytes_to_read;
    let _: fn(&SerialPort<R>, String) -> Result<u32, Error> = SerialPort::<R>::bytes_to_write;
    let _: fn(&SerialPort<R>, String, ClearBuffer) -> Result<(), Error> =
        SerialPort::<R>::clear_buffer;
    let _: fn(&SerialPort<R>, String) -> Result<(), Error> = SerialPort::<R>::set_break;
    let _: fn(&SerialPort<R>, String) -> Result<(), Error> = SerialPort::<R>::clear_break;
    let _: fn(
        &SerialPort<R>,
        String,
        String,
        ExchangeOptions,
    ) -> Result<tauri_plugin_serialplugin::at::parse::ExchangeResponse, Error> =
        SerialPort::<R>::exchange;
    let _: fn(
        &SerialPort<R>,
        String,
        Vec<u8>,
        ExchangeOptions,
    ) -> Result<tauri_plugin_serialplugin::at::parse::ExchangeResponse, Error> =
        SerialPort::<R>::exchange_binary;
    let _: fn(
        &SerialPort<R>,
        String,
        String,
        Option<AtCommandOptions>,
    ) -> Result<tauri_plugin_serialplugin::at::parse::AtCommandResult, Error> = SerialPort::<R>::at;
    let _: fn(
        &SerialPort<R>,
        String,
        Vec<AtPhase>,
    ) -> Result<Vec<tauri_plugin_serialplugin::at::parse::AtCommandResult>, Error> =
        SerialPort::<R>::at_phases;
    let _: fn(
        &SerialPort<R>,
        String,
        u32,
        Vec<u8>,
        Option<SendSmsPduOptions>,
    ) -> Result<Vec<tauri_plugin_serialplugin::at::parse::AtCommandResult>, Error> =
        SerialPort::<R>::send_sms_pdu;
    let _: fn(&SerialPort<R>, String, AtSessionConfig) -> Result<(), Error> =
        SerialPort::<R>::configure_at_session;
    let _: fn(&SerialPort<R>, String, String, u64) -> Result<(), Error> =
        SerialPort::<R>::enable_mux;
    let _: fn(&SerialPort<R>, String, u8) -> Result<String, Error> =
        SerialPort::<R>::open_mux_channel;
    let _: fn(&SerialPort<R>, String) -> Result<(), Error> = SerialPort::<R>::disable_mux;
    let _: fn(&SerialPort<R>, String) -> Result<(), Error> = SerialPort::<R>::cancel_exchange;
    let _: fn(&SerialPort<R>, WatchPortsOptions, Channel<PortListEvent>) -> Result<u32, Error> =
        SerialPort::<R>::watch_ports;
    let _: fn(&SerialPort<R>, String, WatchOptions, Channel<SerialEvent>) -> Result<u32, Error> =
        SerialPort::<R>::watch;
}

/// Given the 4.0.0 `SerialPort<R>` method list
/// When each method is bound to its 4.0.0 function type
/// Then it compiles (path calls such as `SerialPort::<R>::open(&s, ..)` keep working)
#[test]
fn serial_port_methods_keep_their_4_0_0_signatures() {
    assert_signatures::<tauri::test::MockRuntime>();
}
