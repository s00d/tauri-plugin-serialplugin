//! Serial over Bluetooth LE: `ble://<device id or name>` using the Nordic UART
//! Service (NUS), the de-facto "BLE serial" profile of nRF, ESP32, HM-10-style
//! modules and Adafruit/BBC boards.
//!
//! Built on `btleplug` behind the `ble` cargo feature (desktop + iOS). Bytes written
//! go to the NUS RX characteristic in MTU-sized chunks; notifications on the TX
//! characteristic become read data. Line settings and modem lines are stored only:
//! a BLE UART has no baud rate or RTS/DTR.

#[cfg(target_os = "ios")]
use crate::serialport;
use btleplug::api::{
    Central, CentralEvent, Characteristic, Manager as _, Peripheral as _, ScanFilter, WriteType,
};
use btleplug::platform::{Adapter, Manager, Peripheral};
use futures::StreamExt;
use serialport::{ClearBuffer, DataBits, FlowControl, Parity, SerialPort, StopBits};
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use uuid::Uuid;

pub const BLE_SCHEME: &str = "ble://";
pub const NUS_SERVICE: Uuid = Uuid::from_u128(0x6E400001_B5A3_F393_E0A9_E50E24DCCA9E);
/// Central writes here.
pub const NUS_RX: Uuid = Uuid::from_u128(0x6E400002_B5A3_F393_E0A9_E50E24DCCA9E);
/// Peripheral notifies here.
pub const NUS_TX: Uuid = Uuid::from_u128(0x6E400003_B5A3_F393_E0A9_E50E24DCCA9E);

/// `ble://<id>` → `<id>`, or `None` for other paths.
pub fn ble_target(path: &str) -> Option<&str> {
    path.strip_prefix(BLE_SCHEME)
}

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

async fn first_adapter() -> io::Result<Adapter> {
    let manager = Manager::new().await.map_err(ble_err)?;
    manager
        .adapters()
        .await
        .map_err(ble_err)?
        .into_iter()
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no Bluetooth adapter"))
}

/// Scan for devices advertising the Nordic UART Service.
/// Returns `ble://<id>` → port-info map (same shape as `available_ports`).
pub fn scan(duration: Duration) -> io::Result<HashMap<String, HashMap<String, String>>> {
    runtime().block_on(async {
        let adapter = first_adapter().await?;
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
                ("serial_number".to_string(), p.id().to_string()),
                ("vid".to_string(), crate::state::UNKNOWN.to_string()),
                ("pid".to_string(), crate::state::UNKNOWN.to_string()),
                (
                    "rssi".to_string(),
                    props.rssi.map(|r| r.to_string()).unwrap_or_default(),
                ),
            ]);
            out.insert(format!("{BLE_SCHEME}{}", p.id()), info);
        }
        Ok(out)
    })
}

/// Notification chunks → `Read` with the serial timeout semantics the hub expects.
struct RxQueue {
    rx: Receiver<Vec<u8>>,
    pending: Vec<u8>,
}

impl RxQueue {
    fn read(&mut self, buf: &mut [u8], timeout: Duration) -> io::Result<usize> {
        if self.pending.is_empty() {
            match self.rx.recv_timeout(timeout.max(Duration::from_millis(1))) {
                Ok(chunk) => self.pending = chunk,
                Err(RecvTimeoutError::Timeout) => {
                    return Err(io::Error::new(io::ErrorKind::TimedOut, "BLE read timeout"))
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "BLE device disconnected",
                    ))
                }
            }
        }
        let n = buf.len().min(self.pending.len());
        buf[..n].copy_from_slice(&self.pending[..n]);
        self.pending.drain(..n);
        Ok(n)
    }

    fn clear(&mut self) {
        self.pending.clear();
        while self.rx.try_recv().is_ok() {}
    }

    fn buffered(&self) -> usize {
        self.pending.len()
    }
}

pub struct BleSerialPort {
    path: String,
    peripheral: Peripheral,
    rx_char: Characteristic,
    write_type: WriteType,
    rx: Mutex<RxQueue>,
    connected: Arc<AtomicBool>,
    baud_rate: u32,
    data_bits: DataBits,
    flow_control: FlowControl,
    parity: Parity,
    stop_bits: StopBits,
    timeout: Duration,
}

/// Bytes per ATT write: MTU minus the 3-byte ATT header.
// ponytail: chunks are sized from the negotiated MTU but sent one by one with no
// flow control; add credit-based pacing if a peripheral drops write-without-response bursts.
fn chunk_size(mtu: u16) -> usize {
    usize::from(mtu.max(23)) - 3
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
            let adapter = first_adapter().await?;
            let peripheral = find_peripheral(&adapter, &target, Duration::from_secs(10)).await?;
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
            let write_type = if rx_char
                .properties
                .contains(btleplug::api::CharPropFlags::WRITE_WITHOUT_RESPONSE)
            {
                WriteType::WithoutResponse
            } else {
                WriteType::WithResponse
            };
            peripheral.subscribe(&tx_char).await.map_err(ble_err)?;

            let (tx, rx) = mpsc::channel();
            let connected = Arc::new(AtomicBool::new(true));
            let mut notes = peripheral.notifications().await.map_err(ble_err)?;
            let mut events = adapter.events().await.map_err(ble_err)?;
            let id = peripheral.id();
            let alive = connected.clone();
            // Forward notifications until the device disconnects; dropping `tx`
            // then turns the next read into UnexpectedEof (→ hub Disconnect).
            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        n = notes.next() => match n {
                            Some(n) if n.uuid == NUS_TX => {
                                if tx.send(n.value).is_err() { break; }
                            }
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

            Ok(Self {
                path: path.to_string(),
                peripheral,
                rx_char,
                write_type,
                rx: Mutex::new(RxQueue {
                    rx,
                    pending: Vec::new(),
                }),
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

async fn find_peripheral(
    adapter: &Adapter,
    target: &str,
    limit: Duration,
) -> io::Result<Peripheral> {
    adapter
        .start_scan(ScanFilter {
            services: vec![NUS_SERVICE],
        })
        .await
        .map_err(ble_err)?;
    let deadline = Instant::now() + limit;
    let found = 'scan: loop {
        for p in adapter.peripherals().await.map_err(ble_err)? {
            let by_name = || async {
                matches!(p.properties().await, Ok(Some(props)) if props.local_name.as_deref() == Some(target))
            };
            if p.id().to_string() == target || by_name().await {
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
            format!("no BLE device '{target}' advertising the Nordic UART Service"),
        )
    })
}

impl Drop for BleSerialPort {
    fn drop(&mut self) {
        let p = self.peripheral.clone();
        // Best effort: let the device advertise again for the next open.
        runtime().spawn(async move {
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
        let (p, c, t) = (&self.peripheral, &self.rx_char, self.write_type);
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
    fn rx_queue_splits_chunks_and_reports_timeout_and_disconnect() {
        let (tx, rx) = mpsc::channel();
        let mut q = RxQueue {
            rx,
            pending: Vec::new(),
        };
        tx.send(b"hello".to_vec()).unwrap();
        let mut buf = [0u8; 3];
        assert_eq!(q.read(&mut buf, Duration::from_millis(10)).unwrap(), 3);
        assert_eq!(&buf, b"hel");
        assert_eq!(q.buffered(), 2);
        assert_eq!(q.read(&mut buf, Duration::from_millis(10)).unwrap(), 2);
        assert_eq!(&buf[..2], b"lo");
        let e = q.read(&mut buf, Duration::from_millis(5)).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::TimedOut);
        drop(tx);
        let e = q.read(&mut buf, Duration::from_millis(5)).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::UnexpectedEof);
        q.clear();
    }

    #[test]
    fn write_chunks_follow_mtu() {
        assert_eq!(chunk_size(23), 20);
        assert_eq!(chunk_size(0), 20);
        assert_eq!(chunk_size(247), 244);
        assert_eq!(ble_target("ble://AA:BB"), Some("AA:BB"));
        assert_eq!(ble_target("/dev/ttyUSB0"), None);
    }
}
