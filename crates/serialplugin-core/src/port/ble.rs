//! Serial over Bluetooth LE: `ble://<device id, MAC or name>` using the Nordic UART
//! Service (NUS), the de-facto "BLE serial" profile of nRF, ESP32 and Adafruit/BBC
//! boards. (HM-10 / CC254x modules use service FFE0 instead and are not supported.)
//!
//! Built on `btleplug` behind the `ble` cargo feature (desktop + iOS). Bytes written
//! go to the NUS RX characteristic in MTU-sized chunks; notifications on the TX
//! characteristic become read data. Line settings and modem lines are stored only:
//! a BLE UART has no baud rate or RTS/DTR.

#[cfg(target_os = "ios")]
use crate::serialport;
use btleplug::api::{
    Central, CentralEvent, CentralState, CharPropFlags, Characteristic, Manager as _,
    Peripheral as _, ScanFilter, WriteType,
};
use btleplug::platform::{Adapter, Manager, Peripheral};
use futures::StreamExt;
use serialport::{ClearBuffer, DataBits, FlowControl, Parity, SerialPort, StopBits};
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use uuid::Uuid;

pub use super::ble_rx::{ble_target, BLE_SCHEME};
use super::ble_rx::{chunk_size, rx_channel, target_matches, RxQueue, WritePacer, RX_QUEUE_CAP};
pub const NUS_SERVICE: Uuid = Uuid::from_u128(0x6E400001_B5A3_F393_E0A9_E50E24DCCA9E);
/// Central writes here.
pub const NUS_RX: Uuid = Uuid::from_u128(0x6E400002_B5A3_F393_E0A9_E50E24DCCA9E);
/// Peripheral notifies here.
pub const NUS_TX: Uuid = Uuid::from_u128(0x6E400003_B5A3_F393_E0A9_E50E24DCCA9E);

/// One runtime for all BLE work: btleplug's platform event loops live in it, and
/// the sync serial API drives it with `block_on`.
fn runtime() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .thread_name("serialplugin-ble")
            .enable_all()
            .build()
            .expect("BLE runtime")
    })
}

fn ble_err(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(format!("BLE: {e}"))
}

/// The whole `open()` (connect, service discovery, subscribe) must finish in this
/// time: CoreBluetooth never times out a connect to an absent device by itself.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

/// How long a new adapter may stay in state Unknown before the first scan.
const POWER_ON_WAIT: Duration = Duration::from_secs(2);

/// One adapter for the process (one long-lived `CBCentralManager` on Apple, as Apple
/// advises; a fresh one ignores scans until it reports its state).
async fn adapter() -> io::Result<Adapter> {
    static ADAPTER: OnceLock<Adapter> = OnceLock::new();
    let adapter = match ADAPTER.get() {
        Some(a) => a.clone(),
        None => {
            let manager = Manager::new().await.map_err(ble_err)?;
            let first = manager
                .adapters()
                .await
                .map_err(ble_err)?
                .into_iter()
                .next()
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no Bluetooth adapter"))?;
            // A concurrent first call may have won; keep the stored one.
            ADAPTER.get_or_init(|| first).clone()
        }
    };
    let deadline = Instant::now() + POWER_ON_WAIT;
    loop {
        match adapter.adapter_state().await.map_err(ble_err)? {
            CentralState::PoweredOn => return Ok(adapter),
            CentralState::PoweredOff => {
                return Err(io::Error::new(
                    io::ErrorKind::NotConnected,
                    "Bluetooth is off",
                ))
            }
            CentralState::Unknown if Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(50)).await
            }
            CentralState::Unknown => {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "Bluetooth is not available (off, or the app is not authorized to use it)",
                ))
            }
        }
    }
}

/// Port id of a scanned device: the MAC where the platform has one (Linux's own id is
/// a D-Bus path), else the platform id (a per-host UUID on Apple).
fn port_id(p: &Peripheral) -> String {
    if cfg!(target_os = "linux") {
        p.address().to_string()
    } else {
        p.id().to_string()
    }
}

/// Scan for devices advertising the Nordic UART Service.
/// Returns `ble://<id>` → port-info map (same shape as `available_ports`).
pub fn scan(duration: Duration) -> io::Result<HashMap<String, HashMap<String, String>>> {
    runtime().block_on(async {
        let adapter = adapter().await?;
        adapter
            .start_scan(ScanFilter {
                services: vec![NUS_SERVICE],
            })
            .await
            .map_err(ble_err)?;
        tokio::time::sleep(duration).await;
        let _ = adapter.stop_scan().await;
        let mut out = HashMap::new();
        for p in adapter.peripherals().await.map_err(ble_err)? {
            let Ok(Some(props)) = p.properties().await else {
                continue;
            };
            if !props.services.contains(&NUS_SERVICE) {
                continue;
            }
            let name = props
                .local_name
                .clone()
                .unwrap_or_else(|| crate::state::UNKNOWN.to_string());
            let info = HashMap::from([
                ("type".to_string(), "BLE".to_string()),
                ("product".to_string(), name),
                (
                    "manufacturer".to_string(),
                    crate::state::UNKNOWN.to_string(),
                ),
                ("serial_number".to_string(), port_id(&p)),
                ("vid".to_string(), crate::state::UNKNOWN.to_string()),
                ("pid".to_string(), crate::state::UNKNOWN.to_string()),
                (
                    "rssi".to_string(),
                    props.rssi.map(|r| r.to_string()).unwrap_or_default(),
                ),
            ]);
            out.insert(format!("{BLE_SCHEME}{}", port_id(&p)), info);
        }
        Ok(out)
    })
}

pub struct BleSerialPort {
    path: String,
    peripheral: Peripheral,
    rx_char: Characteristic,
    tx_char: Characteristic,
    pacer: WritePacer,
    rx: Mutex<RxQueue>,
    connected: Arc<AtomicBool>,
    baud_rate: u32,
    data_bits: DataBits,
    flow_control: FlowControl,
    parity: Parity,
    stop_bits: StopBits,
    timeout: Duration,
}

impl BleSerialPort {
    #[allow(clippy::too_many_arguments)]
    pub fn connect(
        path: &str,
        baud_rate: u32,
        data_bits: DataBits,
        flow_control: FlowControl,
        parity: Parity,
        stop_bits: StopBits,
        timeout: Duration,
    ) -> io::Result<Self> {
        let target = ble_target(path)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "not a ble:// path"))?
            .to_string();
        runtime().block_on(async {
            let adapter = adapter().await?;
            let peripheral = find_peripheral(&adapter, &target, Duration::from_secs(10)).await?;
            let opened =
                tokio::time::timeout(CONNECT_TIMEOUT, open_nus(&adapter, &peripheral, &target))
                    .await;
            let (rx_char, tx_char, rx, connected) = match opened {
                Ok(Ok(v)) => v,
                Ok(Err(e)) => {
                    let _ = peripheral.disconnect().await;
                    return Err(e);
                }
                Err(_) => {
                    // Cancels the pending connect (CoreBluetooth keeps trying otherwise).
                    let _ = peripheral.disconnect().await;
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        format!(
                            "BLE open of {target} timed out after {} s",
                            CONNECT_TIMEOUT.as_secs()
                        ),
                    ));
                }
            };
            Ok(Self {
                path: path.to_string(),
                peripheral,
                rx_char,
                tx_char,
                pacer: WritePacer::default(),
                rx: Mutex::new(rx),
                connected,
                baud_rate,
                data_bits,
                flow_control,
                parity,
                stop_bits,
                timeout,
            })
        })
    }
}

/// Connect, find the NUS characteristics and start notifications. The notification
/// and event streams are taken before subscribing, so nothing sent right after the
/// CCCD write is lost.
async fn open_nus(
    adapter: &Adapter,
    peripheral: &Peripheral,
    target: &str,
) -> io::Result<(Characteristic, Characteristic, RxQueue, Arc<AtomicBool>)> {
    peripheral.connect().await.map_err(ble_err)?;
    peripheral.discover_services().await.map_err(ble_err)?;
    let chars = peripheral.characteristics();
    let find = |uuid| {
        chars
            .iter()
            .find(|c| c.uuid == uuid)
            .cloned()
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("{target} has no Nordic UART characteristic {uuid}"),
                )
            })
    };
    let rx_char = find(NUS_RX)?;
    let tx_char = find(NUS_TX)?;
    let mut notes = peripheral.notifications().await.map_err(ble_err)?;
    let mut events = adapter.events().await.map_err(ble_err)?;
    // btleplug enables notify, or indicate when TX only indicates.
    peripheral.subscribe(&tx_char).await.map_err(ble_err)?;

    let (feed, rx) = rx_channel(RX_QUEUE_CAP);
    let connected = Arc::new(AtomicBool::new(true));
    let id = peripheral.id();
    let alive = connected.clone();
    // Forward notifications until the device disconnects; dropping `feed` then turns
    // the next read into UnexpectedEof (→ hub Disconnect).
    tokio::spawn(async move {
        loop {
            tokio::select! {
                n = notes.next() => match n {
                    Some(n) if n.uuid == NUS_TX => { feed.push(n.value); }
                    Some(_) => {}
                    None => break,
                },
                e = events.next() => match e {
                    Some(CentralEvent::DeviceDisconnected(d)) if d == id => break,
                    Some(_) => {}
                    None => break,
                },
            }
        }
        alive.store(false, Ordering::SeqCst);
    });
    Ok((rx_char, tx_char, rx, connected))
}

async fn find_peripheral(
    adapter: &Adapter,
    target: &str,
    limit: Duration,
) -> io::Result<Peripheral> {
    // Unfiltered: many NUS firmwares do not advertise the 128-bit service UUID (the
    // service is checked after connecting anyway).
    adapter
        .start_scan(ScanFilter::default())
        .await
        .map_err(ble_err)?;
    let deadline = Instant::now() + limit;
    let found = 'scan: loop {
        for p in adapter.peripherals().await.map_err(ble_err)? {
            let name = match p.properties().await {
                Ok(Some(props)) => props.local_name,
                _ => None,
            };
            let (id, address) = (p.id().to_string(), p.address().to_string());
            if target_matches(&id, &address, name.as_deref(), target) {
                break 'scan Some(p);
            }
        }
        if Instant::now() >= deadline {
            break None;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    };
    let _ = adapter.stop_scan().await;
    found.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("no BLE device '{target}' found (id, MAC or advertised name)"),
        )
    })
}

impl Drop for BleSerialPort {
    fn drop(&mut self) {
        let (p, tx) = (self.peripheral.clone(), self.tx_char.clone());
        // Best effort: turn notifications off (BlueZ re-enables them on reconnect
        // otherwise) and let the device advertise again for the next open.
        runtime().spawn(async move {
            let _ = p.unsubscribe(&tx).await;
            let _ = p.disconnect().await;
        });
    }
}

impl Read for BleSerialPort {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let timeout = self.timeout;
        crate::sync_util::lock_or_recover(&self.rx).read(buf, timeout)
    }
}

impl Write for BleSerialPort {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if !self.connected.load(Ordering::SeqCst) {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "BLE device disconnected",
            ));
        }
        let n = buf.len().min(chunk_size(self.peripheral.mtu()));
        let props = self.rx_char.properties;
        let t = if self.pacer.with_response(
            props.contains(CharPropFlags::WRITE),
            props.contains(CharPropFlags::WRITE_WITHOUT_RESPONSE),
        ) {
            WriteType::WithResponse
        } else {
            WriteType::WithoutResponse
        };
        let (p, c) = (&self.peripheral, &self.rx_char);
        runtime()
            .block_on(p.write(c, &buf[..n], t))
            .map_err(ble_err)?;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl SerialPort for BleSerialPort {
    fn name(&self) -> Option<String> {
        Some(self.path.clone())
    }
    fn baud_rate(&self) -> serialport::Result<u32> {
        Ok(self.baud_rate)
    }
    fn data_bits(&self) -> serialport::Result<DataBits> {
        Ok(self.data_bits)
    }
    fn flow_control(&self) -> serialport::Result<FlowControl> {
        Ok(self.flow_control)
    }
    fn parity(&self) -> serialport::Result<Parity> {
        Ok(self.parity)
    }
    fn stop_bits(&self) -> serialport::Result<StopBits> {
        Ok(self.stop_bits)
    }
    fn timeout(&self) -> Duration {
        self.timeout
    }
    fn set_baud_rate(&mut self, v: u32) -> serialport::Result<()> {
        self.baud_rate = v;
        Ok(())
    }
    fn set_data_bits(&mut self, v: DataBits) -> serialport::Result<()> {
        self.data_bits = v;
        Ok(())
    }
    fn set_flow_control(&mut self, v: FlowControl) -> serialport::Result<()> {
        self.flow_control = v;
        Ok(())
    }
    fn set_parity(&mut self, v: Parity) -> serialport::Result<()> {
        self.parity = v;
        Ok(())
    }
    fn set_stop_bits(&mut self, v: StopBits) -> serialport::Result<()> {
        self.stop_bits = v;
        Ok(())
    }
    fn set_timeout(&mut self, v: Duration) -> serialport::Result<()> {
        self.timeout = v;
        Ok(())
    }
    fn write_request_to_send(&mut self, _: bool) -> serialport::Result<()> {
        Ok(())
    }
    fn write_data_terminal_ready(&mut self, _: bool) -> serialport::Result<()> {
        Ok(())
    }
    fn read_clear_to_send(&mut self) -> serialport::Result<bool> {
        Ok(true)
    }
    fn read_data_set_ready(&mut self) -> serialport::Result<bool> {
        Ok(self.connected.load(Ordering::SeqCst))
    }
    fn read_ring_indicator(&mut self) -> serialport::Result<bool> {
        Ok(false)
    }
    fn read_carrier_detect(&mut self) -> serialport::Result<bool> {
        Ok(self.connected.load(Ordering::SeqCst))
    }
    fn bytes_to_read(&self) -> serialport::Result<u32> {
        Ok(crate::sync_util::lock_or_recover(&self.rx).buffered() as u32)
    }
    fn bytes_to_write(&self) -> serialport::Result<u32> {
        Ok(0)
    }
    fn clear(&self, which: ClearBuffer) -> serialport::Result<()> {
        if !matches!(which, ClearBuffer::Output) {
            crate::sync_util::lock_or_recover(&self.rx).clear();
        }
        Ok(())
    }
    fn try_clone(&self) -> serialport::Result<Box<dyn SerialPort>> {
        Err(serialport::Error::new(
            serialport::ErrorKind::Unknown,
            "a BLE port cannot be cloned",
        ))
    }
    fn set_break(&self) -> serialport::Result<()> {
        Ok(())
    }
    fn clear_break(&self) -> serialport::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_chunks_follow_mtu() {
        assert_eq!(chunk_size(247), 244);
        assert_eq!(chunk_size(517), 512);
        assert_eq!(ble_target("ble://AA:BB"), Some("AA:BB"));
        assert_eq!(ble_target("/dev/ttyUSB0"), None);
    }
}
