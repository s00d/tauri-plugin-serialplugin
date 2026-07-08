//! Rust USB driver host (android-usb-serial + fd from Kotlin).

#[cfg(target_os = "android")]
use crate::android::fd_bridge::{call_close_device_fd, call_open_device_fd};
#[cfg(target_os = "android")]
use crate::android::usb_path;
#[cfg(target_os = "android")]
use crate::error::Error;
#[cfg(target_os = "android")]
use crate::state::{DataBits, FlowControl, Parity, StopBits};
#[cfg(target_os = "android")]
use android_usb_serial::config::{
    DataBits as UsbDataBits, FlowControl as UsbFlowControl, LineConfig, Parity as UsbParity,
    StopBits as UsbStopBits,
};
#[cfg(target_os = "android")]
use android_usb_serial::device::{describe_device, open_port};
#[cfg(target_os = "android")]
use android_usb_serial::serialport_compat::SerialPortAdapter;
#[cfg(target_os = "android")]
use android_usb_serial::transport::SharedTransport;
#[cfg(all(target_os = "android", feature = "android-test-harness"))]
use android_usb_serial::FakeTransport;
#[cfg(target_os = "android")]
use android_usb_serial::NusbTransport;
#[cfg(target_os = "android")]
use serialport::SerialPort;
#[cfg(target_os = "android")]
use std::collections::HashMap;
#[cfg(target_os = "android")]
use std::sync::{Arc, Mutex, OnceLock};

#[cfg(target_os = "android")]
struct PortHost {
    adapter: SerialPortAdapter,
}

#[cfg(target_os = "android")]
struct DeviceHost {
    transport: SharedTransport,
}

#[cfg(target_os = "android")]
pub struct DriverHost {
    devices: Mutex<HashMap<String, DeviceHost>>,
    ports: Mutex<HashMap<String, PortHost>>,
    #[cfg(feature = "android-test-harness")]
    fake_devices: Mutex<HashMap<String, Arc<FakeTransport>>>,
}

#[cfg(target_os = "android")]
impl DriverHost {
    pub fn new() -> Self {
        Self::default()
    }
}

#[cfg(target_os = "android")]
impl Default for DriverHost {
    fn default() -> Self {
        Self {
            devices: Mutex::new(HashMap::new()),
            ports: Mutex::new(HashMap::new()),
            #[cfg(feature = "android-test-harness")]
            fake_devices: Mutex::new(HashMap::new()),
        }
    }
}

#[cfg(target_os = "android")]
impl DriverHost {
    fn map_line(baud: u32, data_bits: DataBits, parity: Parity, stop_bits: StopBits) -> LineConfig {
        LineConfig {
            baud_rate: baud,
            data_bits: match data_bits {
                DataBits::Five => UsbDataBits::Five,
                DataBits::Six => UsbDataBits::Six,
                DataBits::Seven => UsbDataBits::Seven,
                DataBits::Eight => UsbDataBits::Eight,
            },
            parity: match parity {
                Parity::None => UsbParity::None,
                Parity::Odd => UsbParity::Odd,
                Parity::Even => UsbParity::Even,
            },
            stop_bits: match stop_bits {
                StopBits::One => UsbStopBits::One,
                StopBits::Two => UsbStopBits::Two,
            },
        }
    }

    fn map_flow(flow: FlowControl) -> UsbFlowControl {
        match flow {
            FlowControl::None => UsbFlowControl::None,
            FlowControl::Hardware => UsbFlowControl::RtsCts,
            FlowControl::Software => UsbFlowControl::XonXoff,
        }
    }

    fn ensure_device(&self, device_name: &str) -> Result<SharedTransport, Error> {
        let mut devices = self.devices.lock().map_err(|e| Error::new(e.to_string()))?;
        if let Some(entry) = devices.get(device_name) {
            return Ok(entry.transport.clone());
        }

        #[cfg(feature = "android-test-harness")]
        if let Some(fake) = self
            .fake_devices
            .lock()
            .map_err(|e| Error::new(e.to_string()))?
            .get(device_name)
            .cloned()
        {
            let transport: SharedTransport = fake;
            devices.insert(
                device_name.to_string(),
                DeviceHost {
                    transport: transport.clone(),
                },
            );
            return Ok(transport);
        }

        crate::log_info!("[SerialOpen] openDeviceFd {device_name} …");
        let fd = call_open_device_fd(device_name).map_err(|e| {
            crate::log_error!("[SerialOpen] openDeviceFd {device_name}: {e}");
            e
        })?;
        if fd < 0 {
            return Err(Error::new(format!("openDeviceFd failed for {device_name}")));
        }
        crate::log_info!("[SerialOpen] from_raw_fd fd={fd} device={device_name}");
        let device = android_usb_serial::from_raw_fd(fd).map_err(|e| {
            crate::log_error!("[SerialOpen] from_raw_fd: {e}");
            Error::new(e.to_string())
        })?;
        let transport: SharedTransport =
            Arc::new(NusbTransport::from_device(device).map_err(|e| {
                crate::log_error!("[SerialOpen] NusbTransport: {e}");
                Error::new(e.to_string())
            })?);
        devices.insert(
            device_name.to_string(),
            DeviceHost {
                transport: transport.clone(),
            },
        );
        Ok(transport)
    }

    /// Open USB port and return a [`serialport::SerialPort`] facade (RX via reader + ring).
    pub fn open(
        &self,
        path: &str,
        baud_rate: u32,
        data_bits: DataBits,
        flow_control: FlowControl,
        parity: Parity,
        stop_bits: StopBits,
    ) -> Result<(String, Box<dyn SerialPort>), Error> {
        let (device_name, port_index) = usb_path::parse(path);
        crate::log_info!("[SerialOpen] open path={path} device={device_name} port={port_index}");
        let transport = self.ensure_device(device_name)?;
        let desc = describe_device(&transport).map_err(|e| {
            crate::log_error!("[SerialOpen] describe_device: {e}");
            Error::new(e.to_string())
        })?;
        crate::log_info!(
            "[SerialOpen] probed {} port(s) vid={:04x} pid={:04x}",
            desc.ports.len(),
            desc.vendor_id,
            desc.product_id
        );
        let session_path = usb_path::session_key(device_name, port_index, desc.ports.len());
        self.close(Some(&session_path))?;

        let mut handle = open_port(transport, port_index).map_err(|e| {
            crate::log_error!("[SerialOpen] open_port({port_index}): {e}");
            Error::new(e.to_string())
        })?;
        let line_config = Self::map_line(baud_rate, data_bits, parity, stop_bits);
        let usb_flow = Self::map_flow(flow_control);
        handle.set_line_config(line_config).map_err(|e| {
            crate::log_error!("[SerialOpen] set_line_config: {e}");
            Error::new(e.to_string())
        })?;
        handle.set_flow_control(usb_flow).map_err(|e| {
            crate::log_error!("[SerialOpen] set_flow_control: {e}");
            Error::new(e.to_string())
        })?;
        let _ = handle.set_dtr(true);
        let _ = handle.set_rts(true);

        let adapter = SerialPortAdapter::new(handle, session_path.clone(), line_config, usb_flow)
            .map_err(|e| Error::new(e.to_string()))?;
        adapter.start_reader().map_err(|e| {
            crate::log_error!("[SerialOpen] start_reader: {e}");
            Error::new(e.to_string())
        })?;
        crate::log_info!("[SerialOpen] ok session={session_path}");
        let port: Box<dyn SerialPort> = Box::new(adapter.clone());

        self.ports
            .lock()
            .map_err(|e| Error::new(e.to_string()))?
            .insert(session_path.clone(), PortHost { adapter });

        Ok((session_path, port))
    }

    pub fn close(&self, path: Option<&str>) -> Result<(), Error> {
        let mut ports = self.ports.lock().map_err(|e| Error::new(e.to_string()))?;
        let keys: Vec<String> = match path {
            Some(p) => vec![p.to_string()],
            None => ports.keys().cloned().collect(),
        };
        for key in keys {
            if let Some(host) = ports.remove(&key) {
                // Teardown: stop reader → driver close → drop port (device fd closed separately).
                host.adapter.shutdown();
            }
        }
        if path.is_none() {
            let device_names: Vec<String> = self
                .devices
                .lock()
                .map_err(|e| Error::new(e.to_string()))?
                .keys()
                .cloned()
                .collect();
            self.devices
                .lock()
                .map_err(|e| Error::new(e.to_string()))?
                .clear();
            for name in device_names {
                let _ = call_close_device_fd(&name);
            }
        }
        Ok(())
    }

    pub fn on_device_detached(&self, device_name: &str) {
        let paths: Vec<String> = self
            .ports
            .lock()
            .ok()
            .map(|ports| {
                ports
                    .keys()
                    .filter(|p| usb_path::parse(p).0 == device_name)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        for path in paths {
            let _ = self.close(Some(&path));
            crate::android::registry::on_usb_error(
                &path,
                "USB device detached (unplug, power loss, or protocol error on bulk IN)",
            );
        }
        let _ = self.devices.lock().map(|mut d| d.remove(device_name));
        let _ = call_close_device_fd(device_name);
    }

    #[cfg(feature = "android-test-harness")]
    pub fn inject_fake_device(&self, device_name: &str, transport: Arc<FakeTransport>) {
        self.fake_devices
            .lock()
            .expect("fake_devices")
            .insert(device_name.to_string(), transport);
    }

    #[cfg(feature = "android-test-harness")]
    pub fn fake_transport(&self, device_name: &str) -> Option<Arc<FakeTransport>> {
        self.fake_devices.lock().ok()?.get(device_name).cloned()
    }

    pub fn write(&self, path: &str, data: &[u8]) -> Result<usize, Error> {
        crate::log_info!("[SerialWrite] path={path} len={}", data.len());
        let ports = self.ports.lock().map_err(|e| Error::new(e.to_string()))?;
        let host = ports
            .get(path)
            .ok_or_else(|| Error::new(format!("port not open: {path}")))?;
        use std::io::Write;
        let mut port: Box<dyn SerialPort> = Box::new(host.adapter.clone());
        port.write_all(data).map_err(|e| {
            crate::log_error!("[SerialWrite] failed path={path}: {e}");
            Error::new(e.to_string())
        })?;
        crate::log_info!("[SerialWrite] ok path={path} written={}", data.len());
        Ok(data.len())
    }
}

#[cfg(target_os = "android")]
static HOST: OnceLock<Arc<DriverHost>> = OnceLock::new();

#[cfg(target_os = "android")]
pub fn global_host() -> Arc<DriverHost> {
    HOST.get_or_init(|| Arc::new(DriverHost::new())).clone()
}
