//! Mobile serial API — Rust orchestration with thin Kotlin USB layer.

use crate::android::usb_io::MobileUsbIo;
use crate::api::serial_port::{
    configure_at_session as configure_at_session_shared, run_exchange_queued,
};
use crate::at::session::{AtCommandOptions, AtPhase, AtSessionConfig, SendSmsPduOptions};
use crate::cmux::{mux_path, parse_mux_path, CmuxSession, MobileCmuxIo};
use crate::error::Error;
use crate::events::{ExchangeOptions, SerialEvent, WatchOptions, WatchPortsOptions};
use crate::hub::mobile::MobileRxHub;
use crate::hub::RxHubHandle;
use crate::port::tx_queue::PortTxQueue;
use crate::state::{
    ClearBuffer, DataBits, FlowControl, MobileConnectedPort, MobileConnectedPortHandle,
    MobilePortState, MobileSerialportInfo, Parity, StopBits, VirtualPortRef,
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
    crate::android::registry::global_registry().unregister(path);
    if let Ok(mut guard) = cp.rx_hub.lock() {
        if let Some(hub) = guard.take() {
            hub.shutdown_hub();
        }
    }
    cp.tx_queue.cancel_all();
    cp.tx_queue.clear_halt();
}

fn ensure_mobile_rx_hub(cp: &MobileConnectedPortHandle) -> Result<(), Error> {
    let mut guard = cp.rx_hub.lock().map_err(|e| lock_err(e))?;
    if guard.is_none() {
        let hub = Arc::new(MobileRxHub::new(cp.path.clone()));
        let hub_handle: Arc<dyn crate::hub::RxHubHandle> = hub.clone();
        crate::android::registry::global_registry().register(hub_handle, cp.clone());
        *guard = Some(hub);
    }
    Ok(())
}

/// Access to serial port APIs on Android (Rust-first).
pub struct SerialPort<R: Runtime> {
    io: MobileUsbIo,
    _runtime: PhantomData<fn() -> R>,
    ports: Arc<Mutex<HashMap<String, MobileSerialportInfo>>>,
    virtual_ports: Arc<Mutex<HashMap<String, VirtualPortRef>>>,
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
        crate::android::registry::set_rust_state_fail(Box::new(move |path, _reason| {
            if let Err(e) = Self::rust_fail_port_state(&ports, &virtual_ports, path) {
                crate::log_warn!("rust_fail_port_state {}: {}", path, e);
            }
        }));
        crate::port::list_monitor::set_android_enumerator({
            let io_enum = self.io.clone();
            move |single| io_enum.available_ports(single)
        });
        crate::android::registry::init_registry();
    }

    /// Drop managed-port state after USB failure (Kotlin closes the device on main thread).
    fn rust_fail_port_state(
        ports: &Arc<Mutex<HashMap<String, MobileSerialportInfo>>>,
        virtual_ports: &Arc<Mutex<HashMap<String, VirtualPortRef>>>,
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
        for channel_id in crate::port::watch_registry::paths_for_port(&path) {
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
        for channel_id in crate::port::watch_registry::paths_for_port(&path) {
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
        crate::port::watch_registry::register(channel_id, path.clone())?;

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
            crate::port::watch_registry::unregister(channel_id);
            return Err(e);
        }

        Ok(channel_id)
    }

    pub fn unwatch(&self, channel_id: u32) -> Result<(), Error> {
        let path = crate::port::watch_registry::unregister(channel_id)
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
        crate::port::list_monitor::subscribe(channel_id, channel, options)?;
        Ok(channel_id)
    }

    pub fn unwatch_ports(&self, channel_id: u32) -> Result<(), Error> {
        crate::port::list_monitor::unsubscribe(channel_id);
        Ok(())
    }

    pub fn exchange(
        &self,
        path: String,
        value: String,
        options: ExchangeOptions,
    ) -> Result<crate::at::parse::ExchangeResponse, Error> {
        self.exchange_bytes(path, value.into_bytes(), options)
    }

    pub fn exchange_binary(
        &self,
        path: String,
        value: Vec<u8>,
        options: ExchangeOptions,
    ) -> Result<crate::at::parse::ExchangeResponse, Error> {
        self.exchange_bytes(path, value, options)
    }

    fn exchange_bytes(
        &self,
        path: String,
        payload: Vec<u8>,
        options: ExchangeOptions,
    ) -> Result<crate::at::parse::ExchangeResponse, Error> {
        run_exchange_queued(
            path,
            options,
            |p| self.get_tx_queue(p),
            |physical_path,
             dlci,
             virtual_path,
             payload,
             options|
             -> Result<crate::at::parse::ExchangeResponse, Error> {
                self.exchange_bytes_mux_direct(physical_path, dlci, virtual_path, payload, options)
            },
            |path, payload, options| -> Result<crate::at::parse::ExchangeResponse, Error> {
                self.exchange_bytes_direct(path, payload, options)
            },
            payload,
        )
    }

    fn exchange_bytes_mux_direct(
        &self,
        physical_path: String,
        _dlci: u8,
        virtual_path: String,
        payload: Vec<u8>,
        options: ExchangeOptions,
    ) -> Result<crate::at::parse::ExchangeResponse, Error> {
        let vp = self
            .virtual_ports
            .lock()
            .map_err(|e| lock_err(e))?
            .get(&virtual_path)
            .cloned()
            .ok_or_else(|| Error::String(format!("Virtual port '{}' not open", virtual_path)))?;
        self.exchange_bytes_mux_via_ref(physical_path, &vp, payload, options)
    }

    fn exchange_bytes_mux_via_ref(
        &self,
        physical_path: String,
        vp: &VirtualPortRef,
        payload: Vec<u8>,
        options: ExchangeOptions,
    ) -> Result<crate::at::parse::ExchangeResponse, Error> {
        let dlci = vp.dlci;
        let command = options
            .command
            .clone()
            .unwrap_or_else(|| String::from_utf8_lossy(&payload).into_owned());
        let user_solicited = options.solicited_prefixes.clone().unwrap_or_default();

        let session = {
            let cp = self.with_connected_port(physical_path.clone(), |h| Ok(h))?;
            let mux = cp.mux.lock().map_err(|e| lock_err(e))?.clone();
            mux.ok_or_else(|| Error::String("CMUX not enabled on physical port".into()))?
        };

        ensure_mobile_rx_hub(&self.with_connected_port(physical_path, |h| Ok(h))?)?;

        crate::exchange::run::run_mux_exchange(
            &session,
            dlci,
            &command,
            &user_solicited,
            payload,
            options,
            vp.exchange_cancel.clone(),
        )
    }

    fn run_exchange_unqueued(
        &self,
        path: String,
        payload: Vec<u8>,
        options: ExchangeOptions,
    ) -> Result<crate::at::parse::ExchangeResponse, Error> {
        if let Some((physical, dlci)) = parse_mux_path(path.as_str()) {
            return self.exchange_bytes_mux_direct(
                physical.to_string(),
                dlci,
                path,
                payload,
                options,
            );
        }
        self.exchange_bytes_direct(path, payload, options)
    }

    fn exchange_bytes_direct(
        &self,
        path: String,
        payload: Vec<u8>,
        options: ExchangeOptions,
    ) -> Result<crate::at::parse::ExchangeResponse, Error> {
        let command = options
            .command
            .clone()
            .unwrap_or_else(|| String::from_utf8_lossy(&payload).into_owned());
        let user_solicited = options.solicited_prefixes.clone().unwrap_or_default();

        let cp = self.with_connected_port(path.clone(), |h| Ok(h))?;
        ensure_mobile_rx_hub(&cp)?;

        struct MobileExchangeIo<'a, R: Runtime> {
            port: &'a SerialPort<R>,
            path: String,
        }
        impl<R: Runtime> crate::exchange::io::ExchangeIo for MobileExchangeIo<'_, R> {
            fn purge_rx(&self) -> Result<(), Error> {
                self.port.io.clear_buffer(&self.path, ClearBuffer::Input)
            }
            fn write_payload(&self, payload: &[u8]) -> Result<(), Error> {
                self.port
                    .io
                    .write(&self.path, payload)
                    .map(|_| ())
                    .map_err(|e| e)
            }
        }

        let guard = cp.rx_hub.lock().map_err(|e| lock_err(e))?;
        let hub = guard
            .as_ref()
            .ok_or_else(|| Error::String("RX hub missing".into()))?;

        crate::exchange::run::run_physical_exchange(
            hub.as_ref(),
            &MobileExchangeIo {
                port: self,
                path: path.clone(),
            },
            &command,
            &user_solicited,
            payload,
            options,
            cp.exchange_cancel.clone(),
        )
    }

    pub fn cancel_exchange(&self, path: String) -> Result<(), Error> {
        if let Ok(virtuals) = self.virtual_ports.lock() {
            if let Some(vp) = virtuals.get(&path).cloned() {
                drop(virtuals);
                let session = self.with_connected_port(vp.physical_path.clone(), |cp| {
                    Ok(cp.mux.lock().map_err(|e| lock_err(e))?.clone())
                })?;
                if let Some(session) = session {
                    crate::exchange::cancel::cancel_virtual_exchange(
                        &vp.exchange_cancel,
                        &vp.tx_queue,
                        &session,
                        vp.dlci,
                    );
                } else {
                    vp.exchange_cancel.store(true, Ordering::SeqCst);
                    vp.tx_queue.cancel_all();
                    vp.tx_queue.clear_halt();
                }
                return Ok(());
            }
        }
        self.with_connected_port(path, |cp| {
            crate::exchange::cancel::cancel_physical_exchange(
                &cp.exchange_cancel,
                &cp.tx_queue,
                || {
                    if let Ok(guard) = cp.rx_hub.lock() {
                        if let Some(hub) = guard.as_ref() {
                            hub.cancel_active_exchange();
                        }
                    }
                },
            );
            Ok(())
        })
    }

    pub fn at(
        &self,
        path: String,
        command: String,
        options: Option<AtCommandOptions>,
    ) -> Result<crate::at::parse::AtCommandResult, Error> {
        let tx_queue = self.get_tx_queue(&path)?;
        crate::at::commands::queue_at(self, &tx_queue, path, command, options)
    }

    pub fn at_phases(
        &self,
        path: String,
        phases: Vec<AtPhase>,
    ) -> Result<Vec<crate::at::parse::AtCommandResult>, Error> {
        let tx_queue = self.get_tx_queue(&path)?;
        crate::at::commands::queue_at_phases(self, &tx_queue, path, phases)
    }

    pub fn send_sms_pdu(
        &self,
        path: String,
        length: u32,
        pdu: Vec<u8>,
        options: Option<SendSmsPduOptions>,
    ) -> Result<Vec<crate::at::parse::AtCommandResult>, Error> {
        let tx_queue = self.get_tx_queue(&path)?;
        crate::at::commands::queue_send_sms_pdu(self, &tx_queue, path, length, pdu, options)
    }

    pub fn configure_at_session(
        &self,
        path: String,
        session: AtSessionConfig,
    ) -> Result<(), Error> {
        let tx_queue = self.get_tx_queue(&path).ok();
        configure_at_session_shared(&self.virtual_ports, tx_queue, &path, session)
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
            if crate::sync_util::lock_or_recover(&cp.mux).is_some() {
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
        let vp = VirtualPortRef {
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

impl<R: Runtime> crate::at::commands::ExchangeRunner for SerialPort<R> {
    fn run_exchange_unqueued(
        &self,
        path: String,
        payload: Vec<u8>,
        options: ExchangeOptions,
    ) -> Result<crate::at::parse::ExchangeResponse, Error> {
        SerialPort::run_exchange_unqueued(self, path, payload, options)
    }
}
