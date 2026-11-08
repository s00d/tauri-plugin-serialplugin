//! Tauri-managed `SerialPort<R>`: a thin layer over [`serialplugin_core::SerialManager`].
//!
//! The 4.0.0 methods are inherent (same signatures, delegating to the core), so path
//! calls such as `SerialPort::<R>::open(&s, ..)` keep working; newer core methods are
//! reachable through `Deref`. The streaming calls take a Tauri IPC [`Channel`] (whose
//! id becomes the watch id) instead of a sink.

use crate::at::session::{AtCommandOptions, AtPhase, AtSessionConfig, SendSmsPduOptions};
use crate::error::Error;
use crate::events::{
    channel_sink, ExchangeOptions, PortListEvent, SerialEvent, WatchOptions, WatchPortsOptions,
};
use crate::state::{ClearBuffer, DataBits, FlowControl, Parity, StopBits};
use serialplugin_core::SerialManager;
use std::collections::HashMap;
use std::ops::Deref;
use std::time::Duration;
use tauri::ipc::Channel;
#[cfg(not(target_os = "android"))]
use tauri::plugin::PluginHandle;
#[cfg(not(target_os = "android"))]
use tauri::AppHandle;
use tauri::Runtime;

/// Access to the serial port APIs (desktop + Android).
pub struct SerialPort<R: Runtime> {
    #[cfg(not(target_os = "android"))]
    #[allow(dead_code)]
    pub(crate) app: AppHandle<R>,
    #[cfg(target_os = "android")]
    _runtime: std::marker::PhantomData<fn() -> R>,
    pub(crate) core: SerialManager,
}

impl<R: Runtime> Clone for SerialPort<R> {
    fn clone(&self) -> Self {
        Self {
            #[cfg(not(target_os = "android"))]
            app: self.app.clone(),
            #[cfg(target_os = "android")]
            _runtime: std::marker::PhantomData,
            core: self.core.clone(),
        }
    }
}

impl<R: Runtime> Deref for SerialPort<R> {
    type Target = SerialManager;

    fn deref(&self) -> &SerialManager {
        &self.core
    }
}

#[cfg(target_os = "android")]
impl<R: Runtime> Default for SerialPort<R> {
    fn default() -> Self {
        Self {
            _runtime: std::marker::PhantomData,
            core: SerialManager::new(),
        }
    }
}

impl<R: Runtime> SerialPort<R> {
    #[cfg(target_os = "android")]
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(not(target_os = "android"))]
    pub fn new(app: AppHandle<R>) -> Self {
        Self {
            app,
            core: SerialManager::new(),
        }
    }

    #[cfg(not(target_os = "android"))]
    #[allow(dead_code)]
    pub fn from_plugin_handle(plugin_handle: PluginHandle<R>) -> Self {
        Self::new(plugin_handle.app().clone())
    }

    /// The Tauri-free core (share it with code that does not know about Tauri).
    pub fn core(&self) -> &SerialManager {
        &self.core
    }

    /// Subscribe to available-port attach/detach events via a Tauri channel.
    pub fn watch_ports(
        &self,
        options: WatchPortsOptions,
        channel: Channel<PortListEvent>,
    ) -> Result<u32, Error> {
        self.core
            .watch_ports_with_id(channel.id(), options, channel_sink(channel))
    }

    /// Stream port data through a Tauri IPC channel. Returns the channel id for `unwatch`.
    pub fn watch(
        &self,
        path: String,
        options: WatchOptions,
        channel: Channel<SerialEvent>,
    ) -> Result<u32, Error> {
        self.core
            .watch_with_id(channel.id(), path, options, channel_sink(channel))
    }

    /// [`Self::watch`] with a native parser (see [`SerialManager::watch_framed`]).
    pub fn watch_framed(
        &self,
        path: String,
        options: WatchOptions,
        parser: Option<serial_codec::framing::FramerConfig>,
        channel: Channel<SerialEvent>,
    ) -> Result<u32, Error> {
        self.core
            .watch_framed_with_id(channel.id(), path, options, parser, channel_sink(channel))
    }
}

/// The 4.0.0 `SerialPort` API, delegating to [`SerialManager`].
impl<R: Runtime> SerialPort<R> {
    #[cfg(target_os = "android")]
    pub fn setup_teardown(&self) {
        self.core.setup_teardown()
    }

    /// Get serial port list
    pub fn available_ports(
        &self,
        single_port_per_device: bool,
    ) -> Result<HashMap<String, HashMap<String, String>>, Error> {
        self.core.available_ports(single_port_per_device)
    }

    /// Stop a port-list watch session.
    pub fn unwatch_ports(&self, channel_id: u32) -> Result<(), Error> {
        self.core.unwatch_ports(channel_id)
    }

    /// Get a list of managed serial ports.
    pub fn managed_ports(&self) -> Result<Vec<String>, Error> {
        self.core.managed_ports()
    }

    /// Cancel an in-flight poll [`Self::read`] (does not stop an active [`Self::watch`]).
    pub fn cancel_read(&self, path: String) -> Result<(), Error> {
        self.core.cancel_read(path)
    }

    /// Close the specified serial port
    pub fn close(&self, path: String) -> Result<(), Error> {
        self.core.close(path)
    }

    /// Close all open serial ports
    pub fn close_all(&self) -> Result<(), Error> {
        self.core.close_all()
    }

    /// Force close a serial port
    pub fn force_close(&self, path: String) -> Result<(), Error> {
        self.core.force_close(path)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn open(
        &self,
        path: String,
        baud_rate: u32,
        data_bits: Option<DataBits>,
        flow_control: Option<FlowControl>,
        parity: Option<Parity>,
        stop_bits: Option<StopBits>,
        timeout: Option<u64>,
    ) -> Result<String, Error> {
        self.core.open(
            path,
            baud_rate,
            data_bits,
            flow_control,
            parity,
            stop_bits,
            timeout,
        )
    }

    /// [`Self::open`] with explicit DTR / RTS levels on open.
    #[allow(clippy::too_many_arguments)]
    pub fn open_with_lines(
        &self,
        path: String,
        baud_rate: u32,
        data_bits: Option<DataBits>,
        flow_control: Option<FlowControl>,
        parity: Option<Parity>,
        stop_bits: Option<StopBits>,
        timeout: Option<u64>,
        lines: crate::state::ModemLinesOnOpen,
    ) -> Result<String, Error> {
        self.core.open_with_lines(
            path,
            baud_rate,
            data_bits,
            flow_control,
            parity,
            stop_bits,
            timeout,
            lines,
        )
    }

    pub fn unwatch(&self, channel_id: u32) -> Result<(), Error> {
        self.core.unwatch(channel_id)
    }

    pub fn read(
        &self,
        path: String,
        timeout: Option<u64>,
        size: Option<usize>,
    ) -> Result<String, Error> {
        self.core.read(path, timeout, size)
    }

    pub fn read_binary(
        &self,
        path: String,
        timeout: Option<u64>,
        size: Option<usize>,
    ) -> Result<Vec<u8>, Error> {
        self.core.read_binary(path, timeout, size)
    }

    /// Write data to the serial port
    pub fn write(&self, path: String, value: String) -> Result<usize, Error> {
        self.core.write(path, value)
    }

    /// Write binary data to the serial port
    pub fn write_binary(&self, path: String, value: Vec<u8>) -> Result<usize, Error> {
        self.core.write_binary(path, value)
    }

    /// Set the baud rate
    pub fn set_baud_rate(&self, path: String, baud_rate: u32) -> Result<(), Error> {
        self.core.set_baud_rate(path, baud_rate)
    }

    /// Set the data bits
    pub fn set_data_bits(&self, path: String, data_bits: DataBits) -> Result<(), Error> {
        self.core.set_data_bits(path, data_bits)
    }

    /// Set the flow control
    pub fn set_flow_control(&self, path: String, flow_control: FlowControl) -> Result<(), Error> {
        self.core.set_flow_control(path, flow_control)
    }

    /// Set the parity
    pub fn set_parity(&self, path: String, parity: Parity) -> Result<(), Error> {
        self.core.set_parity(path, parity)
    }

    /// Set the stop bits
    pub fn set_stop_bits(&self, path: String, stop_bits: StopBits) -> Result<(), Error> {
        self.core.set_stop_bits(path, stop_bits)
    }

    /// Set the timeout
    pub fn set_timeout(&self, path: String, timeout: Duration) -> Result<(), Error> {
        self.core.set_timeout(path, timeout)
    }

    /// Set the RTS (Request To Send) control signal
    pub fn write_request_to_send(&self, path: String, level: bool) -> Result<(), Error> {
        self.core.write_request_to_send(path, level)
    }

    /// Set the DTR (Data Terminal Ready) control signal
    pub fn write_data_terminal_ready(&self, path: String, level: bool) -> Result<(), Error> {
        self.core.write_data_terminal_ready(path, level)
    }

    /// Read the CTS (Clear To Send) control signal state
    pub fn read_clear_to_send(&self, path: String) -> Result<bool, Error> {
        self.core.read_clear_to_send(path)
    }

    /// Read the DSR (Data Set Ready) control signal state
    pub fn read_data_set_ready(&self, path: String) -> Result<bool, Error> {
        self.core.read_data_set_ready(path)
    }

    /// Read the RI (Ring Indicator) control signal state
    pub fn read_ring_indicator(&self, path: String) -> Result<bool, Error> {
        self.core.read_ring_indicator(path)
    }

    /// Read the CD (Carrier Detect) control signal state
    pub fn read_carrier_detect(&self, path: String) -> Result<bool, Error> {
        self.core.read_carrier_detect(path)
    }

    /// Get the number of bytes available to read
    pub fn bytes_to_read(&self, path: String) -> Result<u32, Error> {
        self.core.bytes_to_read(path)
    }

    /// Get the number of bytes waiting to be written
    pub fn bytes_to_write(&self, path: String) -> Result<u32, Error> {
        self.core.bytes_to_write(path)
    }

    /// Clear input/output buffers
    pub fn clear_buffer(&self, path: String, buffer_to_clear: ClearBuffer) -> Result<(), Error> {
        self.core.clear_buffer(path, buffer_to_clear)
    }

    /// Start break signal transmission
    pub fn set_break(&self, path: String) -> Result<(), Error> {
        self.core.set_break(path)
    }

    /// Stop break signal transmission
    pub fn clear_break(&self, path: String) -> Result<(), Error> {
        self.core.clear_break(path)
    }

    /// Write payload and read until terminators, idle silence, or timeout (AT-style exchange).
    pub fn exchange(
        &self,
        path: String,
        value: String,
        options: ExchangeOptions,
    ) -> Result<crate::at::parse::ExchangeResponse, Error> {
        self.core.exchange(path, value, options)
    }

    /// Binary variant of [`Self::exchange`].
    pub fn exchange_binary(
        &self,
        path: String,
        value: Vec<u8>,
        options: ExchangeOptions,
    ) -> Result<crate::at::parse::ExchangeResponse, Error> {
        self.core.exchange_binary(path, value, options)
    }

    pub fn at(
        &self,
        path: String,
        command: String,
        options: Option<AtCommandOptions>,
    ) -> Result<crate::at::parse::AtCommandResult, Error> {
        self.core.at(path, command, options)
    }

    /// Multi-phase AT flow (e.g. CMGS) — phases run atomically without interleaving other jobs.
    pub fn at_phases(
        &self,
        path: String,
        phases: Vec<AtPhase>,
    ) -> Result<Vec<crate::at::parse::AtCommandResult>, Error> {
        self.core.at_phases(path, phases)
    }

    /// Built-in CMGS recipe: `AT+CMGS=n` → `>` → PDU + Ctrl+Z → final line.
    pub fn send_sms_pdu(
        &self,
        path: String,
        length: u32,
        pdu: Vec<u8>,
        options: Option<SendSmsPduOptions>,
    ) -> Result<Vec<crate::at::parse::AtCommandResult>, Error> {
        self.core.send_sms_pdu(path, length, pdu, options)
    }

    /// Configure AT session defaults for native `at` jobs on this port.
    pub fn configure_at_session(
        &self,
        path: String,
        session: AtSessionConfig,
    ) -> Result<(), Error> {
        self.core.configure_at_session(path, session)
    }

    /// Enter GSM 07.10 CMUX mode (`AT+CMUX=…` then attach deframer to the RX hub).
    pub fn enable_mux(&self, path: String, command: String, timeout_ms: u64) -> Result<(), Error> {
        self.core.enable_mux(path, command, timeout_ms)
    }

    /// Register a virtual DLCI channel on an CMUX-enabled physical port.
    pub fn open_mux_channel(&self, physical_path: String, dlci: u8) -> Result<String, Error> {
        self.core.open_mux_channel(physical_path, dlci)
    }

    /// Tear down CMUX: close virtual channels and detach deframer.
    pub fn disable_mux(&self, path: String) -> Result<(), Error> {
        self.core.disable_mux(path)
    }

    /// Cancel an in-flight exchange and reject queued waiters on `path`.
    pub fn cancel_exchange(&self, path: String) -> Result<(), Error> {
        self.core.cancel_exchange(path)
    }
}

impl<R: Runtime> serialplugin_core::at::commands::ExchangeRunner for SerialPort<R> {
    fn run_exchange_unqueued(
        &self,
        path: String,
        payload: Vec<u8>,
        options: crate::events::ExchangeOptions,
    ) -> Result<crate::at::parse::ExchangeResponse, Error> {
        serialplugin_core::at::commands::ExchangeRunner::run_exchange_unqueued(
            &self.core, path, payload, options,
        )
    }
}
