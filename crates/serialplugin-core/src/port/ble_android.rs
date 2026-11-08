//! Serial over Bluetooth LE on Android: `ble://<MAC or advertised name>`, Nordic UART
//! Service. btleplug is not used on Android; GATT lives in Kotlin
//! (`BleGattBridge`: connect, MTU 247, notify or indicate on TX, paced chunked writes on RX).
//! This side is the `serialport::SerialPort` adapter:
//!
//! - RX: `MobileBridge.onBleData(handle, bytes)` → [`on_data`] → the port's [`RxQueue`]
//!   (capped at [`RX_QUEUE_CAP`] unread bytes; newer notifications are dropped past it).
//! - Link loss: `MobileBridge.onBleDisconnected` → [`on_disconnected`] drops the queue's
//!   sender, so the next read is `UnexpectedEof` → hub `Disconnect`.
//! - TX / close: [`BleOps`] (JNI on Android, a fake in host unit tests).

#[cfg(target_os = "ios")]
use crate::serialport;

use super::ble_rx::{ble_target, rx_channel, RxFeed, RxQueue, RX_QUEUE_CAP};
use serialport::{ClearBuffer, DataBits, FlowControl, Parity, SerialPort, StopBits};
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

/// Budget of one whole `open()` on the Kotlin side: the name or address scan, connect
/// (and its one retry on 133/62/129), service discovery, MTU, enabling notifications
/// and waiting for pairing. Each step gets `min(its own max, what is left)`.
const CONNECT_TIMEOUT_MS: u64 = 20_000;

/// Platform calls the port needs. `connect(target, handle, timeout_ms)` must not return
/// before notifications are enabled; data for `handle` may arrive right after.
pub struct BleOps {
    pub connect: fn(&str, i64, u64) -> io::Result<()>,
    pub write: fn(i64, &[u8]) -> io::Result<usize>,
    pub close: fn(i64),
}

/// handle → RX sender. Registered *before* connect so no early notification is lost.
static FEEDS: LazyLock<Mutex<HashMap<i64, RxFeed>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
static NEXT_HANDLE: AtomicI64 = AtomicI64::new(1);

fn feeds() -> std::sync::MutexGuard<'static, HashMap<i64, RxFeed>> {
    crate::sync_util::lock_or_recover(&FEEDS)
}

/// Notification bytes from the TX characteristic (JNI, binder thread: never blocks).
pub fn on_data(handle: i64, data: Vec<u8>) {
    if let Some(feed) = feeds().get(&handle) {
        feed.push(data);
    }
}

/// The GATT link dropped (not a close we asked for).
pub fn on_disconnected(handle: i64, reason: &str) {
    if feeds().remove(&handle).is_some() {
        crate::log_info!("[BLE] handle {handle} disconnected: {reason}");
    }
}

fn is_connected(handle: i64) -> bool {
    feeds().contains_key(&handle)
}

/// Kotlin scan JSON (`{"ble://AA:..": {"type": "BLE", ...}}`) → port-info map.
pub fn parse_scan_json(json: &str) -> io::Result<HashMap<String, HashMap<String, String>>> {
    serde_json::from_str(json)
        .map_err(|e| io::Error::other(format!("BLE: invalid scan JSON: {e}; body={json}")))
}

/// Scan for devices advertising the Nordic UART Service (same shape as desktop).
#[cfg(all(target_os = "android", feature = "ble"))]
pub fn scan(duration: Duration) -> io::Result<HashMap<String, HashMap<String, String>>> {
    let ms = u64::try_from(duration.as_millis()).unwrap_or(u64::MAX);
    let json = crate::android::ble_bridge::call_scan_json(ms).map_err(io::Error::other)?;
    parse_scan_json(&json)
}

pub struct AndroidBleSerialPort {
    path: String,
    handle: i64,
    ops: &'static BleOps,
    rx: Mutex<RxQueue>,
    baud_rate: u32,
    data_bits: DataBits,
    flow_control: FlowControl,
    parity: Parity,
    stop_bits: StopBits,
    timeout: Duration,
}

impl AndroidBleSerialPort {
    #[cfg(all(target_os = "android", feature = "ble"))]
    pub fn connect(
        path: &str,
        baud_rate: u32,
        data_bits: DataBits,
        flow_control: FlowControl,
        parity: Parity,
        stop_bits: StopBits,
        timeout: Duration,
    ) -> io::Result<Self> {
        Self::connect_with(
            &crate::android::ble_bridge::JNI_OPS,
            path,
            (baud_rate, data_bits, flow_control, parity, stop_bits),
            timeout,
        )
    }

    fn connect_with(
        ops: &'static BleOps,
        path: &str,
        line: (u32, DataBits, FlowControl, Parity, StopBits),
        timeout: Duration,
    ) -> io::Result<Self> {
        let target = ble_target(path)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "not a ble:// path"))?;
        let handle = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);
        let (feed, rx) = rx_channel(RX_QUEUE_CAP);
        feeds().insert(handle, feed);
        if let Err(e) = (ops.connect)(target, handle, CONNECT_TIMEOUT_MS) {
            feeds().remove(&handle);
            return Err(e);
        }
        let (baud_rate, data_bits, flow_control, parity, stop_bits) = line;
        Ok(Self {
            path: path.to_string(),
            handle,
            ops,
            rx: Mutex::new(rx),
            baud_rate,
            data_bits,
            flow_control,
            parity,
            stop_bits,
            timeout,
        })
    }
}

impl Drop for AndroidBleSerialPort {
    fn drop(&mut self) {
        feeds().remove(&self.handle);
        (self.ops.close)(self.handle);
    }
}

impl Read for AndroidBleSerialPort {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let timeout = self.timeout;
        crate::sync_util::lock_or_recover(&self.rx).read(buf, timeout)
    }
}

impl Write for AndroidBleSerialPort {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if !is_connected(self.handle) {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "BLE device disconnected",
            ));
        }
        if buf.is_empty() {
            return Ok(0);
        }
        (self.ops.write)(self.handle, buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl SerialPort for AndroidBleSerialPort {
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
        Ok(is_connected(self.handle))
    }
    fn read_ring_indicator(&mut self) -> serialport::Result<bool> {
        Ok(false)
    }
    fn read_carrier_detect(&mut self) -> serialport::Result<bool> {
        Ok(is_connected(self.handle))
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

    /// What the fake platform saw: (op, handle, payload).
    static CALLS: Mutex<Vec<(&'static str, i64, Vec<u8>)>> = Mutex::new(Vec::new());

    fn record(op: &'static str, handle: i64, data: &[u8]) {
        CALLS.lock().unwrap().push((op, handle, data.to_vec()));
    }
    fn calls_for(handle: i64) -> Vec<(&'static str, Vec<u8>)> {
        CALLS
            .lock()
            .unwrap()
            .iter()
            .filter(|c| c.1 == handle)
            .map(|c| (c.0, c.2.clone()))
            .collect()
    }

    static OK_OPS: BleOps = BleOps {
        connect: |target, handle, _| {
            record("connect", handle, target.as_bytes());
            // A notification racing the connect return must not be lost.
            on_data(handle, b"hi".to_vec());
            Ok(())
        },
        write: |handle, data| {
            record("write", handle, data);
            Ok(data.len())
        },
        close: |handle| record("close", handle, &[]),
    };

    static FAILED_HANDLE: AtomicI64 = AtomicI64::new(0);
    static FAIL_OPS: BleOps = BleOps {
        connect: |_, handle, _| {
            FAILED_HANDLE.store(handle, Ordering::SeqCst);
            record("connect", handle, &[]);
            Err(io::Error::other("GATT 133"))
        },
        write: |_, _| unreachable!(),
        close: |handle| record("close", handle, &[]),
    };

    fn open(ops: &'static BleOps) -> io::Result<AndroidBleSerialPort> {
        AndroidBleSerialPort::connect_with(
            ops,
            "ble://AA:BB:CC:DD:EE:FF",
            (
                115_200,
                DataBits::Eight,
                FlowControl::None,
                Parity::None,
                StopBits::One,
            ),
            Duration::from_millis(20),
        )
    }

    #[test]
    fn fed_data_reads_writes_go_out_and_drop_closes() {
        let mut port = open(&OK_OPS).unwrap();
        let h = port.handle;
        let mut buf = [0u8; 8];
        assert_eq!(port.read(&mut buf).unwrap(), 2, "early notification kept");
        assert_eq!(&buf[..2], b"hi");
        on_data(h, b"OK\r\n".to_vec());
        assert_eq!(
            port.bytes_to_read().unwrap(),
            4,
            "queued notification counts"
        );
        assert_eq!(port.read(&mut buf).unwrap(), 4);
        assert_eq!(
            port.read(&mut buf).unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
        assert_eq!(port.write(b"AT\r").unwrap(), 3);
        assert_eq!(port.write(b"").unwrap(), 0);
        assert!(port.read_carrier_detect().unwrap());
        drop(port);
        assert!(!is_connected(h));
        assert_eq!(
            calls_for(h),
            vec![
                ("connect", b"AA:BB:CC:DD:EE:FF".to_vec()),
                ("write", b"AT\r".to_vec()),
                ("close", vec![]),
            ]
        );
    }

    #[test]
    fn link_loss_ends_reads_and_writes() {
        let mut port = open(&OK_OPS).unwrap();
        let h = port.handle;
        let mut buf = [0u8; 8];
        assert_eq!(port.read(&mut buf).unwrap(), 2);
        on_disconnected(h, "status 8");
        assert_eq!(
            port.read(&mut buf).unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof,
            "hub maps this to Disconnect"
        );
        assert_eq!(
            port.write(b"x").unwrap_err().kind(),
            io::ErrorKind::BrokenPipe
        );
        assert!(!port.read_data_set_ready().unwrap());
        on_data(h, b"late".to_vec()); // no panic, dropped
    }

    #[test]
    fn failed_connect_unregisters_and_does_not_close() {
        let err = open(&FAIL_OPS).err().unwrap();
        assert!(err.to_string().contains("133"));
        let h = FAILED_HANDLE.load(Ordering::SeqCst);
        assert!(!is_connected(h));
        assert_eq!(
            calls_for(h),
            vec![("connect", vec![])],
            "no close after a failed connect"
        );
        let not_ble = AndroidBleSerialPort::connect_with(
            &OK_OPS,
            "/dev/ttyUSB0",
            (
                9600,
                DataBits::Eight,
                FlowControl::None,
                Parity::None,
                StopBits::One,
            ),
            Duration::from_millis(1),
        );
        assert_eq!(not_ble.err().unwrap().kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn scan_json_parses_to_port_info() {
        let m = parse_scan_json(
            r#"{"ble://AA:BB:CC:DD:EE:FF":{"type":"BLE","product":"Nordic_UART","rssi":"-60"}}"#,
        )
        .unwrap();
        assert_eq!(m["ble://AA:BB:CC:DD:EE:FF"]["type"], "BLE");
        assert_eq!(m["ble://AA:BB:CC:DD:EE:FF"]["rssi"], "-60");
        assert!(parse_scan_json("{}").unwrap().is_empty());
        assert!(parse_scan_json("not json").is_err());
    }
}
