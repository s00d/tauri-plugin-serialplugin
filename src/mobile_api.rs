//! Mobile serial API — Rust orchestration with thin Kotlin USB layer.

use crate::at_session::{
    check_expect_ok, normalize_at_command, AtCommandOptions, AtPhase, AtPhaseWrite,
    AtSessionConfig, SendSmsPduOptions,
};
use crate::cmux::{mux_path, CmuxSession, MobileCmuxIo};
use crate::error::Error;
use crate::events::{ExchangeOptions, SerialEvent, WatchOptions, WatchPortsOptions};
use crate::mobile_registry;
use crate::mobile_rx_hub::MobileRxHub;
use crate::mobile_usb_io::MobileUsbIo;
use crate::port_tx_queue::PortTxQueue;
use crate::state::{
    ClearBuffer, DataBits, FlowControl, MobileConnectedPort, MobileConnectedPortHandle,
    MobilePortState, MobileSerialportInfo, MobileVirtualPortRef, Parity, StopBits,
};
use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::ipc::Channel;
use tauri::Runtime;

const DEFAULT_PORT_TIMEOUT_MS: u64 = 1000;

fn lock_err<T>(e: std::sync::PoisonError<T>) -> Error {
    Error::String(format!("Mutex lock failed: {}", e))
}

fn finish_mobile_port(path: &str, cp: &MobileConnectedPortHandle) {
    mobile_registry::global_registry().unregister(path);
    if let Ok(mut guard) = cp.rx_hub.lock() {
        if let Some(hub) = guard.take() {
            hub.shutdown();
        }
    }
    cp.tx_queue.cancel_all();
    cp.tx_queue.clear_halt();
}

fn ensure_mobile_rx_hub(cp: &MobileConnectedPortHandle) -> Result<(), Error> {
    let mut guard = cp.rx_hub.lock().map_err(|e| lock_err(e))?;
    if guard.is_none() {
        let hub = Arc::new(MobileRxHub::new(cp.path.clone()));
        mobile_registry::global_registry().register(hub.clone(), cp.clone());
        *guard = Some(hub);
    }
    Ok(())
}

/// Access to serial port APIs on Android (Rust-first).
pub struct SerialPort<R: Runtime> {
    io: MobileUsbIo,
    _runtime: PhantomData<fn() -> R>,
    ports: Arc<Mutex<HashMap<String, MobileSerialportInfo>>>,
    virtual_ports: Arc<Mutex<HashMap<String, MobileVirtualPortRef>>>,
}

impl<R: Runtime> SerialPort<R> {
    pub fn new() -> Self {
        Self {
            io: MobileUsbIo::new(),
            _runtime: PhantomData::<fn() -> R>,
            ports: Arc::new(Mutex::new(HashMap::new())),
            virtual_ports: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn setup_teardown(&self) {
        let ports = self.ports.clone();
        let virtual_ports = self.virtual_ports.clone();
        mobile_registry::set_rust_state_fail(Box::new(move |path, _reason| {
            if let Err(e) = Self::rust_fail_port_state(&ports, &virtual_ports, path) {
                crate::log_warn!("rust_fail_port_state {}: {}", path, e);
            }
        }));
        crate::port_list_monitor::set_android_enumerator({
            let io_enum = self.io.clone();
            move |single| io_enum.available_ports(single)
        });
        mobile_registry::init_registry();
    }

    /// Drop managed-port state after USB failure (Kotlin closes the device on main thread).
    fn rust_fail_port_state(
        ports: &Arc<Mutex<HashMap<String, MobileSerialportInfo>>>,
        virtual_ports: &Arc<Mutex<HashMap<String, MobileVirtualPortRef>>>,
        path: &str,
    ) -> Result<(), Error> {
        if let Ok(mut v) = virtual_ports.lock() {
            if let Some(vp) = v.remove(path) {
                vp.tx_queue.cancel_all();
                vp.tx_queue.clear_halt();
            }
        }
        let mut map = ports.lock().map_err(|e| lock_err(e))?;
        if let Some(info) = map.remove(path) {
            let _ = info;
        }
        Ok(())
    }

    fn with_connected_port<F, T>(&self, path: String, f: F) -> Result<T, Error>
    where
        F: FnOnce(MobileConnectedPortHandle) -> Result<T, Error>,
    {
        let handle = {
            let ports = self.ports.lock().map_err(|e| lock_err(e))?;
            match ports.get(&path) {
                Some(MobileSerialportInfo {
                    state: MobilePortState::Connected(cp),
                }) => cp.handle(),
                Some(info) => return Err(Error::String(info.state.not_connected_reason())),
                None => return Err(Error::String(format!("Port '{}' not found", path))),
            }
        };
        f(handle)
    }

    fn get_tx_queue(&self, path: &str) -> Result<Arc<PortTxQueue>, Error> {
        if let Ok(v) = self.virtual_ports.lock() {
            if let Some(vp) = v.get(path) {
                return Ok(vp.tx_queue.clone());
            }
        }
        let ports = self.ports.lock().map_err(|e| lock_err(e))?;
        match ports.get(path).map(|i| &i.state) {
            Some(MobilePortState::Connected(cp)) => Ok(cp.tx_queue.clone()),
            Some(other) => Err(Error::String(other.not_connected_reason())),
            None => Err(Error::String(format!("Port '{}' not found", path))),
        }
    }

    pub fn available_ports(
        &self,
        single_port_per_device: bool,
    ) -> Result<HashMap<String, HashMap<String, String>>, Error> {
        self.io.available_ports(single_port_per_device)
    }

    pub fn managed_ports(&self) -> Result<Vec<String>, Error> {
        Ok(self
            .ports
            .lock()
            .map_err(|e| lock_err(e))?
            .iter()
            .filter_map(|(path, info)| {
                if matches!(info.state, MobilePortState::Connected(_)) {
                    Some(path.clone())
                } else {
                    None
                }
            })
            .collect())
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
        if self.managed_ports()?.contains(&path) {
            let _ = self.close(path.clone());
        }

        {
            let mut ports = self.ports.lock().map_err(|e| lock_err(e))?;
            ports.insert(
                path.clone(),
                MobileSerialportInfo {
                    state: MobilePortState::Opening,
                },
            );
        }

        let result = self.io.open(
            path.clone(),
            baud_rate,
            data_bits.unwrap_or(DataBits::Eight),
            flow_control.unwrap_or(FlowControl::None),
            parity.unwrap_or(Parity::None),
            stop_bits.unwrap_or(StopBits::One),
            timeout.unwrap_or(DEFAULT_PORT_TIMEOUT_MS),
        );

        let mut ports = self.ports.lock().map_err(|e| lock_err(e))?;
        match result {
            Ok(session_path) => {
                if session_path != path {
                    ports.remove(&path);
                }
                if !ports.contains_key(&session_path) {
                    ports.insert(
                        session_path.clone(),
                        MobileSerialportInfo {
                            state: MobilePortState::Opening,
                        },
                    );
                }
                let entry = ports.get_mut(&session_path).ok_or_else(|| {
                    Error::String(format!("Port '{}' disappeared during open", session_path))
                })?;
                entry.state =
                    MobilePortState::Connected(MobileConnectedPort::new(session_path.clone()));
                if let MobilePortState::Connected(cp) = &entry.state {
                    let _ = ensure_mobile_rx_hub(&cp.handle());
                }
                Ok(session_path)
            }
            Err(e) => {
                ports.remove(&path);
                Err(e)
            }
        }
    }

    pub fn close(&self, path: String) -> Result<(), Error> {
        for channel_id in crate::watch_registry::paths_for_port(&path) {
            let _ = self.unwatch(channel_id);
        }

        if let Ok(mut v) = self.virtual_ports.lock() {
            if let Some(vp) = v.remove(&path) {
                vp.tx_queue.cancel_all();
                return self.with_connected_port(vp.physical_path.clone(), |cp| {
                    if let Some(session) = cp.mux.lock().unwrap().clone() {
                        session.unregister_dlci(vp.dlci);
                    }
                    Ok(())
                });
            }
        }

        let cp = {
            let mut ports = self.ports.lock().map_err(|e| lock_err(e))?;
            match ports.remove(&path) {
                Some(info) => match info.state {
                    MobilePortState::Connected(cp) => Some(cp.handle()),
                    _ => None,
                },
                None => return Err(Error::String(format!("Serial port {} is not open!", path))),
            }
        };

        if let Some(cp) = cp {
            finish_mobile_port(&path, &cp);
            self.io.close(&path)?;
        }
        Ok(())
    }

    pub fn close_all(&self) -> Result<(), Error> {
        let paths = self.managed_ports()?;
        let mut errors = vec![];
        for path in paths {
            if let Err(e) = self.close(path) {
                errors.push(e.to_string());
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(Error::String(errors.join(", ")))
        }
    }

    pub fn force_close(&self, path: String) -> Result<(), Error> {
        for channel_id in crate::watch_registry::paths_for_port(&path) {
            let _ = self.unwatch(channel_id);
        }
        let cp = {
            let mut ports = self.ports.lock().map_err(|e| lock_err(e))?;
            ports.remove(&path).and_then(|info| match info.state {
                MobilePortState::Connected(cp) => Some(cp.handle()),
                _ => None,
            })
        };
        if let Some(cp) = cp {
            finish_mobile_port(&path, &cp);
        }
        let _ = self.virtual_ports.lock().map(|mut v| v.remove(&path));
        let _ = self.io.close(&path);
        Ok(())
    }

    pub fn write(&self, path: String, data: String) -> Result<usize, Error> {
        let tx_queue = self.get_tx_queue(&path)?;
        tx_queue.run_serial(|| self.io.write(&path, data.as_bytes()).map_err(|e| e))
    }

    pub fn write_binary(&self, path: String, data: Vec<u8>) -> Result<usize, Error> {
        let tx_queue = self.get_tx_queue(&path)?;
        tx_queue.run_serial(|| self.io.write(&path, &data).map_err(|e| e))
    }

    pub fn read(
        &self,
        path: String,
        timeout: Option<u64>,
        size: Option<usize>,
    ) -> Result<String, Error> {
        let bytes = self.read_via_hub(path, timeout, size, false)?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    pub fn read_binary(
        &self,
        path: String,
        timeout: Option<u64>,
        size: Option<usize>,
    ) -> Result<Vec<u8>, Error> {
        self.read_via_hub(path, timeout, size, true)
    }

    fn read_via_hub(
        &self,
        path: String,
        timeout: Option<u64>,
        size: Option<usize>,
        fill: bool,
    ) -> Result<Vec<u8>, Error> {
        self.with_connected_port(path.clone(), |cp| {
            ensure_mobile_rx_hub(&cp)?;
            let guard = cp.rx_hub.lock().map_err(|e| lock_err(e))?;
            let hub = guard
                .as_ref()
                .ok_or_else(|| Error::String("RX hub missing".into()))?;
            hub.shared()
                .read_request(
                    size.unwrap_or(1024),
                    timeout.unwrap_or(DEFAULT_PORT_TIMEOUT_MS),
                    fill,
                )
                .map_err(Error::String)
        })
    }

    pub fn watch(
        &self,
        path: String,
        options: WatchOptions,
        channel: Channel<SerialEvent>,
    ) -> Result<u32, Error> {
        let channel_id = channel.id();
        crate::watch_registry::register(channel_id, path.clone())?;

        let batch_timeout = options
            .serial_data_flush_interval_ms
            .or(options.timeout)
            .unwrap_or(DEFAULT_PORT_TIMEOUT_MS);
        let read_size = options.size.unwrap_or(1024);

        if let Err(e) = self.with_connected_port(path.clone(), |cp| {
            ensure_mobile_rx_hub(&cp)?;
            if let Ok(guard) = cp.rx_hub.lock() {
                if let Some(hub) = guard.as_ref() {
                    hub.detach_watch();
                }
            }
            let guard = cp.rx_hub.lock().map_err(|e| lock_err(e))?;
            let hub = guard
                .as_ref()
                .ok_or_else(|| Error::String("RX hub missing".into()))?;
            hub.attach_watch(channel, batch_timeout, read_size);
            Ok(())
        }) {
            crate::watch_registry::unregister(channel_id);
            return Err(e);
        }

        Ok(channel_id)
    }

    pub fn unwatch(&self, channel_id: u32) -> Result<(), Error> {
        let path = crate::watch_registry::unregister(channel_id)
            .ok_or_else(|| Error::new(format!("No active watch for channel {}", channel_id)))?;
        self.stop_watch(&path)
    }

    fn stop_watch(&self, path: &str) -> Result<(), Error> {
        self.with_connected_port(path.to_string(), |cp| {
            if let Ok(guard) = cp.rx_hub.lock() {
                if let Some(hub) = guard.as_ref() {
                    hub.detach_watch();
                }
            }
            Ok(())
        })?;
        Ok(())
    }

    pub fn watch_ports(
        &self,
        options: WatchPortsOptions,
        channel: Channel<crate::events::PortListEvent>,
    ) -> Result<u32, Error> {
        let channel_id = channel.id();
        crate::port_list_monitor::subscribe(channel_id, channel, options)?;
        Ok(channel_id)
    }

    pub fn unwatch_ports(&self, channel_id: u32) -> Result<(), Error> {
        crate::port_list_monitor::unsubscribe(channel_id);
        Ok(())
    }

    pub fn exchange(
        &self,
        path: String,
        value: String,
        options: ExchangeOptions,
    ) -> Result<crate::at_parse::ExchangeResponse, Error> {
        self.exchange_bytes(path, value.into_bytes(), options)
    }

    pub fn exchange_binary(
        &self,
        path: String,
        value: Vec<u8>,
        options: ExchangeOptions,
    ) -> Result<crate::at_parse::ExchangeResponse, Error> {
        self.exchange_bytes(path, value, options)
    }

    fn exchange_bytes(
        &self,
        path: String,
        payload: Vec<u8>,
        options: ExchangeOptions,
    ) -> Result<crate::at_parse::ExchangeResponse, Error> {
        let tx_queue = self.get_tx_queue(&path)?;
        tx_queue.run_serial(|| self.exchange_bytes_direct(path, payload, options))
    }

    fn exchange_bytes_direct(
        &self,
        path: String,
        payload: Vec<u8>,
        options: ExchangeOptions,
    ) -> Result<crate::at_parse::ExchangeResponse, Error> {
        let command = options
            .command
            .clone()
            .unwrap_or_else(|| String::from_utf8_lossy(&payload).into_owned());
        let user_solicited = options.solicited_prefixes.clone().unwrap_or_default();
        let mut opts = options;
        if opts.command.is_none() {
            opts.command = Some(command.clone());
        }
        let resolved = opts.resolve();

        let cp = self.with_connected_port(path.clone(), |h| Ok(h))?;
        cp.exchange_cancel.store(false, Ordering::SeqCst);
        let cancel = cp.exchange_cancel.clone();

        ensure_mobile_rx_hub(&cp)?;

        use crate::events::RxPrepareMode;
        match resolved.rx_prepare {
            RxPrepareMode::Purge => {
                self.io.clear_buffer(&path, ClearBuffer::Input)?;
            }
            RxPrepareMode::Drain => {
                // SIOM already feeds the hub during watch; soft-drain is desktop-only.
            }
            RxPrepareMode::None => {}
        }

        let waiter = crate::port_rx_hub::ExchangeWaiter::new(resolved.clone(), cancel.clone());
        let hub_shared = {
            let guard = cp.rx_hub.lock().map_err(|e| lock_err(e))?;
            let hub = guard
                .as_ref()
                .ok_or_else(|| Error::String("RX hub missing".into()))?;
            hub.set_exchange_waiter(waiter.clone());
            hub.shared()
        };

        self.io.write(&path, &payload)?;

        let stale = hub_shared.take_idle_bytes();
        if !stale.is_empty() {
            waiter.push_bytes(&stale);
        }

        let wait_result = waiter.wait(resolved.timeout_ms);
        {
            let guard = cp.rx_hub.lock().map_err(|e| lock_err(e))?;
            if let Some(hub) = guard.as_ref() {
                hub.clear_exchange_waiter();
            }
        }
        let (raw, matched) = wait_result.map_err(Error::String)?;
        let outcome = crate::exchange_read::ReadUntilOutcome { raw, matched };

        Ok(crate::at_parse::ExchangeResponse::from_raw(
            outcome.raw,
            outcome.matched,
            &command,
            &user_solicited,
            resolved.result_format,
        ))
    }

    pub fn cancel_exchange(&self, path: String) -> Result<(), Error> {
        self.with_connected_port(path.clone(), |cp| {
            cp.exchange_cancel.store(true, Ordering::SeqCst);
            if let Ok(guard) = cp.rx_hub.lock() {
                if let Some(hub) = guard.as_ref() {
                    hub.cancel_active_exchange();
                }
            }
            cp.tx_queue.cancel_all();
            cp.tx_queue.clear_halt();
            Ok(())
        })
    }

    pub fn at(
        &self,
        path: String,
        command: String,
        options: Option<AtCommandOptions>,
    ) -> Result<crate::at_parse::AtCommandResult, Error> {
        let tx_queue = self.get_tx_queue(&path)?;
        tx_queue.run_serial(|| {
            let session = tx_queue.at_session();
            let append_cr = options
                .as_ref()
                .and_then(|o| o.append_cr)
                .unwrap_or_else(|| session.append_cr());
            let payload = normalize_at_command(&command, append_cr);
            let exchange_opts = session.merge_exchange(&command, options.as_ref());
            let response = self.exchange_bytes_direct(path, payload.into_bytes(), exchange_opts)?;
            check_expect_ok(
                &session,
                response.status,
                &String::from_utf8_lossy(&response.raw),
            )?;
            Ok(crate::at_parse::AtCommandResult::from_exchange(
                command, response,
            ))
        })
    }

    pub fn at_phases(
        &self,
        path: String,
        phases: Vec<AtPhase>,
    ) -> Result<Vec<crate::at_parse::AtCommandResult>, Error> {
        let tx_queue = self.get_tx_queue(&path)?;
        tx_queue.run_serial(|| self.at_phases_direct(path, phases))
    }

    fn at_phases_direct(
        &self,
        path: String,
        phases: Vec<AtPhase>,
    ) -> Result<Vec<crate::at_parse::AtCommandResult>, Error> {
        let tx_queue = self.get_tx_queue(&path)?;
        let session = tx_queue.at_session();
        let stop_on_error = session.stop_on_error();
        let mut results = Vec::with_capacity(phases.len());
        for (i, phase) in phases.iter().enumerate() {
            #[cfg(target_os = "android")]
            if i > 0 {
                // CH340 resets if the next command lands while a large RX is still draining.
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
            let label = phase.command.clone().unwrap_or_else(|| match &phase.write {
                AtPhaseWrite::Text(s) => s.clone(),
                AtPhaseWrite::Binary(b) => format!("<binary {} bytes>", b.len()),
            });
            let rx_prepare = if i == 0 {
                None
            } else {
                Some(crate::events::RxPrepareMode::None)
            };
            let mut exchange_opts = session.merge_phase(phase, &label);
            if let Some(rp) = rx_prepare {
                exchange_opts.rx_prepare = Some(rp);
            }
            let payload = match &phase.write {
                AtPhaseWrite::Text(s) => normalize_at_command(s, session.append_cr()).into_bytes(),
                AtPhaseWrite::Binary(b) => b.clone(),
            };
            let phase_result = (|| -> Result<crate::at_parse::AtCommandResult, Error> {
                let response = self.exchange_bytes_direct(path.clone(), payload, exchange_opts)?;
                check_expect_ok(
                    &session,
                    response.status,
                    &String::from_utf8_lossy(&response.raw),
                )?;
                Ok(crate::at_parse::AtCommandResult::from_exchange(
                    label.clone(),
                    response,
                ))
            })();
            match phase_result {
                Ok(r) => results.push(r),
                Err(e) => {
                    if stop_on_error {
                        return Err(e);
                    }
                    results.push(crate::at_parse::AtCommandResult::failed(
                        label,
                        e.to_string(),
                    ));
                }
            }
        }
        Ok(results)
    }

    pub fn send_sms_pdu(
        &self,
        path: String,
        length: u32,
        pdu: Vec<u8>,
        options: Option<SendSmsPduOptions>,
    ) -> Result<Vec<crate::at_parse::AtCommandResult>, Error> {
        let tx_queue = self.get_tx_queue(&path)?;
        let session = tx_queue.at_session();
        let timeout_ms = options
            .as_ref()
            .and_then(|o| o.timeout_ms)
            .or(session.default_timeout_ms);
        let result_format = options
            .as_ref()
            .and_then(|o| o.result_format)
            .or(session.result_format);
        let cmd = format!("AT+CMGS={length}");
        let mut payload = pdu;
        payload.push(0x1a);
        let phases = vec![
            AtPhase {
                write: AtPhaseWrite::Text(cmd.clone()),
                completion_mode: Some(crate::events::ExchangeCompletionMode::AtIntermediate),
                result_format,
                timeout_ms,
                command: Some(cmd),
                rx_prepare: None,
            },
            AtPhase {
                write: AtPhaseWrite::Binary(payload),
                completion_mode: Some(crate::events::ExchangeCompletionMode::AtFinalLine),
                result_format,
                timeout_ms,
                command: Some(String::new()),
                rx_prepare: Some(crate::events::RxPrepareMode::None),
            },
        ];
        tx_queue.run_serial(|| self.at_phases_direct(path, phases))
    }

    pub fn configure_at_session(
        &self,
        path: String,
        session: AtSessionConfig,
    ) -> Result<(), Error> {
        if let Ok(v) = self.virtual_ports.lock() {
            if let Some(vp) = v.get(&path) {
                vp.tx_queue.configure_at_session(session);
                return Ok(());
            }
        }
        let tx_queue = self.get_tx_queue(&path)?;
        tx_queue.configure_at_session(session);
        Ok(())
    }

    pub fn enable_mux(&self, path: String, command: String, timeout_ms: u64) -> Result<(), Error> {
        self.exchange(
            path.clone(),
            format!("{command}\r"),
            ExchangeOptions {
                timeout_ms: Some(timeout_ms),
                rx_prepare: Some(crate::events::RxPrepareMode::None),
                ..Default::default()
            },
        )?;

        self.with_connected_port(path.clone(), |cp| {
            if cp.mux.lock().unwrap().is_some() {
                return Err(Error::String("CMUX already enabled".into()));
            }
            let io = self.io.clone();
            let path_clone = path.clone();
            let io_writer = MobileCmuxIo::new(move |data: &[u8]| {
                io.write(&path_clone, data)
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            });
            let session = CmuxSession::new(path.clone(), io_writer);
            ensure_mobile_rx_hub(&cp)?;
            if let Ok(guard) = cp.rx_hub.lock() {
                if let Some(hub) = guard.as_ref() {
                    hub.attach_cmux(session.clone());
                }
            }
            *cp.mux.lock().map_err(|e| lock_err(e))? = Some(session);
            Ok(())
        })
    }

    pub fn open_mux_channel(&self, physical_path: String, dlci: u8) -> Result<String, Error> {
        let virtual_path = mux_path(&physical_path, dlci);
        let session = {
            let cp = self.with_connected_port(physical_path.clone(), |h| Ok(h))?;
            let mux = cp.mux.lock().map_err(|e| lock_err(e))?.clone();
            mux.ok_or_else(|| Error::String("CMUX not enabled".into()))?
        };
        ensure_mobile_rx_hub(&self.with_connected_port(physical_path.clone(), |h| Ok(h))?)?;
        session.register_dlci(dlci, virtual_path.clone());
        let vp = MobileVirtualPortRef {
            physical_path: physical_path.clone(),
            dlci,
            exchange_cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            tx_queue: Arc::new(PortTxQueue::new()),
        };
        self.virtual_ports
            .lock()
            .map_err(|e| lock_err(e))?
            .insert(virtual_path.clone(), vp);
        Ok(virtual_path)
    }

    pub fn disable_mux(&self, path: String) -> Result<(), Error> {
        let mut vpaths: Vec<String> = self
            .virtual_ports
            .lock()
            .map_err(|e| lock_err(e))?
            .iter()
            .filter(|(_, vp)| vp.physical_path == path)
            .map(|(p, _)| p.clone())
            .collect();
        for vp in vpaths.drain(..) {
            let _ = self.close(vp);
        }
        self.with_connected_port(path.clone(), |cp| {
            *cp.mux.lock().map_err(|e| lock_err(e))? = None;
            Ok(())
        })
    }

    pub fn set_baud_rate(&self, path: String, baud_rate: u32) -> Result<(), Error> {
        self.io.set_baud_rate(&path, baud_rate)
    }

    pub fn set_data_bits(&self, path: String, data_bits: DataBits) -> Result<(), Error> {
        self.io.set_data_bits(&path, data_bits)
    }

    pub fn set_flow_control(&self, path: String, flow_control: FlowControl) -> Result<(), Error> {
        self.io.set_flow_control(&path, flow_control)
    }

    pub fn set_parity(&self, path: String, parity: Parity) -> Result<(), Error> {
        self.io.set_parity(&path, parity)
    }

    pub fn set_stop_bits(&self, path: String, stop_bits: StopBits) -> Result<(), Error> {
        self.io.set_stop_bits(&path, stop_bits)
    }

    pub fn set_timeout(&self, path: String, timeout: Duration) -> Result<(), Error> {
        self.io.set_timeout(&path, timeout)
    }

    pub fn write_request_to_send(&self, path: String, level: bool) -> Result<(), Error> {
        self.io.write_rts(&path, level)
    }

    pub fn write_data_terminal_ready(&self, path: String, level: bool) -> Result<(), Error> {
        self.io.write_dtr(&path, level)
    }

    pub fn cancel_read(&self, path: String) -> Result<(), Error> {
        self.with_connected_port(path, |cp| {
            ensure_mobile_rx_hub(&cp)?;
            if let Ok(guard) = cp.rx_hub.lock() {
                if let Some(hub) = guard.as_ref() {
                    hub.shared().cancel_pending_read();
                }
            }
            Ok(())
        })
    }

    pub fn read_clear_to_send(&self, path: String) -> Result<bool, Error> {
        self.io.read_cts(&path)
    }

    pub fn read_data_set_ready(&self, path: String) -> Result<bool, Error> {
        self.io.read_dsr(&path)
    }

    pub fn read_ring_indicator(&self, path: String) -> Result<bool, Error> {
        self.io.read_ri(&path)
    }

    pub fn read_carrier_detect(&self, path: String) -> Result<bool, Error> {
        self.io.read_cd(&path)
    }

    pub fn bytes_to_read(&self, path: String) -> Result<u32, Error> {
        self.with_connected_port(path.clone(), |cp| {
            ensure_mobile_rx_hub(&cp)?;
            if let Ok(guard) = cp.rx_hub.lock() {
                if let Some(hub) = guard.as_ref() {
                    return Ok(hub.buffered_len() as u32);
                }
            }
            Ok(0)
        })
    }

    /// Returns pending host TX bytes; Android USB always reports `0` (no kernel queue).
    pub fn bytes_to_write(&self, path: String) -> Result<u32, Error> {
        self.io.bytes_to_write(&path)
    }

    pub fn clear_buffer(&self, path: String, buffer_type: ClearBuffer) -> Result<(), Error> {
        self.io.clear_buffer(&path, buffer_type)
    }

    pub fn set_break(&self, path: String) -> Result<(), Error> {
        self.io.set_break(&path)
    }

    pub fn clear_break(&self, path: String) -> Result<(), Error> {
        self.io.clear_break(&path)
    }
}

impl<R: Runtime> Clone for SerialPort<R> {
    fn clone(&self) -> Self {
        Self {
            io: self.io,
            _runtime: PhantomData::<fn() -> R>,
            ports: self.ports.clone(),
            virtual_ports: self.virtual_ports.clone(),
        }
    }
}
