//! In-memory transport for golden parity and on-device harness.
//!
//! Enable with Cargo feature `fake-transport`. Script control/bulk responses, then open a port
//! with [`crate::open_port`] exactly as on hardware.

use crate::error::{ReadOutcome, Result, UsbSerialError};
use crate::transport::{BulkIn, BulkOut, ControlRequest, EndpointInfo, InterfaceInfo, Transport};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// Captured control transfer (for golden parity assertions).
#[derive(Debug, Clone)]
pub struct RecordedControl {
    pub request_type: u8,
    pub request: u8,
    pub value: u16,
    pub index: u16,
    pub data: Vec<u8>,
}

/// Captured bulk OUT payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedBulkOut {
    pub endpoint: u8,
    pub data: Vec<u8>,
}

#[derive(Debug, Default)]
struct FakeState {
    device_descriptor: [u8; 18],
    raw_descriptors: Vec<u8>,
    interfaces: Vec<InterfaceInfo>,
    endpoints: Vec<(u8, EndpointInfo)>,
    recorded: Vec<RecordedControl>,
    recorded_bulk_out: Vec<RecordedBulkOut>,
    control_in_responses: Vec<Vec<u8>>,
    interrupt_in_queue: VecDeque<u8>,
    rx_queue: VecDeque<u8>,
    tx_log: Vec<u8>,
    bulk_read_error: Option<String>,
    claimed: Vec<u8>,
    /// Mimic nusb: endpoint address may only be opened once until dropped.
    open_endpoints: Vec<u8>,
}

/// Thread-safe fake USB device for tests ([`crate::Transport`] implementor).
#[derive(Debug, Clone)]
pub struct FakeTransport {
    inner: Arc<Mutex<FakeState>>,
}

impl Default for FakeTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeTransport {
    /// Same as [`Self::cdc_single_iface`].
    pub fn new() -> Self {
        Self::cdc_single_iface()
    }

    /// Single CDC ACM interface with bulk IN/OUT endpoints (castrated single-iface layout).
    pub fn cdc_single_iface() -> Self {
        let t = Self {
            inner: Arc::new(Mutex::new(FakeState::default())),
        };
        {
            let mut s = t.inner.lock().unwrap();
            s.device_descriptor = [
                18, 1, 0x00, 0x02, 0x02, 0x00, 0x00, 64, 0x34, 0x12, 0x78, 0x56, 0x00, 0x01, 0x01,
                0x02, 0x00, 1,
            ];
            s.interfaces = vec![InterfaceInfo {
                id: 0,
                class: 2,
                subclass: 2,
                protocol: 0,
            }];
            s.endpoints = vec![
                (
                    0,
                    EndpointInfo {
                        address: 0x81,
                        attributes: 2,
                        max_packet_size: 64,
                        interval: 0,
                    },
                ),
                (
                    0,
                    EndpointInfo {
                        address: 0x02,
                        attributes: 2,
                        max_packet_size: 64,
                        interval: 0,
                    },
                ),
            ];
        }
        t
    }

    /// FT232R-shaped single-interface layout (`0403:6001`).
    pub fn ftdi_ft232r() -> Self {
        let t = Self::cdc_single_iface();
        t.set_vendor_product(0x0403, 0x6001);
        t.set_interfaces(vec![InterfaceInfo {
            id: 0,
            class: 255,
            subclass: 0,
            protocol: 0,
        }]);
        t.configure_endpoints(&[(
            0,
            vec![
                EndpointInfo {
                    address: 0x81,
                    attributes: 2,
                    max_packet_size: 64,
                    interval: 0,
                },
                EndpointInfo {
                    address: 0x02,
                    attributes: 2,
                    max_packet_size: 64,
                    interval: 0,
                },
            ],
        )]);
        t
    }

    pub fn ftdi_ft2232() -> Self {
        let t = Self::ftdi_ft232r();
        t.set_vendor_product(0x0403, 0x6010);
        t.set_interfaces(vec![
            InterfaceInfo {
                id: 0,
                class: 255,
                subclass: 0,
                protocol: 0,
            },
            InterfaceInfo {
                id: 1,
                class: 255,
                subclass: 0,
                protocol: 0,
            },
        ]);
        t.configure_endpoints(&[
            (
                0,
                vec![
                    EndpointInfo {
                        address: 0x81,
                        attributes: 2,
                        max_packet_size: 64,
                        interval: 0,
                    },
                    EndpointInfo {
                        address: 0x02,
                        attributes: 2,
                        max_packet_size: 64,
                        interval: 0,
                    },
                ],
            ),
            (
                1,
                vec![
                    EndpointInfo {
                        address: 0x83,
                        attributes: 2,
                        max_packet_size: 64,
                        interval: 0,
                    },
                    EndpointInfo {
                        address: 0x04,
                        attributes: 2,
                        max_packet_size: 64,
                        interval: 0,
                    },
                ],
            ),
        ]);
        t.patch_device_descriptor(|d| d[13] = 7);
        t
    }

    pub fn cp2102() -> Self {
        Self::ftdi_ft232r().also(|t| t.set_vendor_product(0x10C4, 0xEA60))
    }

    pub fn cp2105() -> Self {
        let t = Self::cp2102();
        t.set_vendor_product(0x10C4, 0xEA70);
        t.set_interfaces(vec![
            InterfaceInfo {
                id: 0,
                class: 255,
                subclass: 0,
                protocol: 0,
            },
            InterfaceInfo {
                id: 1,
                class: 255,
                subclass: 0,
                protocol: 0,
            },
        ]);
        t.configure_endpoints(&[
            (
                0,
                vec![
                    EndpointInfo {
                        address: 0x81,
                        attributes: 2,
                        max_packet_size: 64,
                        interval: 0,
                    },
                    EndpointInfo {
                        address: 0x02,
                        attributes: 2,
                        max_packet_size: 64,
                        interval: 0,
                    },
                ],
            ),
            (
                1,
                vec![
                    EndpointInfo {
                        address: 0x83,
                        attributes: 2,
                        max_packet_size: 64,
                        interval: 0,
                    },
                    EndpointInfo {
                        address: 0x04,
                        attributes: 2,
                        max_packet_size: 64,
                        interval: 0,
                    },
                ],
            ),
        ]);
        t
    }

    pub fn ch340_dual_iface() -> Self {
        let t = Self::cdc_single_iface();
        t.set_vendor_product(0x1A86, 0x7523);
        t.set_interfaces(vec![
            InterfaceInfo {
                id: 0,
                class: 255,
                subclass: 0,
                protocol: 0,
            },
            InterfaceInfo {
                id: 1,
                class: 255,
                subclass: 0,
                protocol: 0,
            },
        ]);
        t.configure_endpoints(&[(
            1,
            vec![
                EndpointInfo {
                    address: 0x82,
                    attributes: 2,
                    max_packet_size: 64,
                    interval: 0,
                },
                EndpointInfo {
                    address: 0x03,
                    attributes: 2,
                    max_packet_size: 64,
                    interval: 0,
                },
            ],
        )]);
        t
    }

    pub fn pl2303_hx() -> Self {
        let t = Self::cdc_single_iface();
        t.set_vendor_product(0x067B, 0x2303);
        t.patch_device_descriptor(|d| d[4] = 0);
        t.configure_endpoints(&[(
            0,
            vec![
                EndpointInfo {
                    address: 0x81,
                    attributes: 3,
                    max_packet_size: 64,
                    interval: 1,
                },
                EndpointInfo {
                    address: 0x02,
                    attributes: 2,
                    max_packet_size: 64,
                    interval: 0,
                },
                EndpointInfo {
                    address: 0x83,
                    attributes: 2,
                    max_packet_size: 64,
                    interval: 0,
                },
            ],
        )]);
        t.set_interfaces(vec![InterfaceInfo {
            id: 0,
            class: 255,
            subclass: 0,
            protocol: 0,
        }]);
        t
    }

    pub fn pl2303_hxn() -> Self {
        let t = Self::pl2303_hx();
        t.patch_device_descriptor(|d| {
            d[4] = 0x00;
            d[7] = 64;
            d[2] = 0x00;
            d[3] = 0x02;
        });
        t
    }

    pub fn pl2303_type01() -> Self {
        let t = Self::pl2303_hx();
        t.patch_device_descriptor(|d| d[4] = 0x02);
        t
    }

    pub fn pl2303_ta() -> Self {
        let t = Self::pl2303_hx();
        t.patch_device_descriptor(|d| {
            d[2] = 0x00;
            d[3] = 0x02;
            d[12] = 0x00;
            d[13] = 0x03;
        });
        t
    }

    pub fn cdc_iad() -> Self {
        let t = Self::cdc_single_iface();
        t.set_vendor_product(0x2341, 0x0043);
        t.patch_device_descriptor(|d| {
            d[4] = 0xEF;
            d[5] = 0x02;
            d[6] = 0x01;
        });
        let i0 = InterfaceInfo {
            id: 0,
            class: 2,
            subclass: 2,
            protocol: 0,
        };
        let i1 = InterfaceInfo {
            id: 1,
            class: 10,
            subclass: 0,
            protocol: 0,
        };
        t.set_interfaces(vec![i0, i1]);
        t.configure_endpoints(&[
            (
                0,
                vec![EndpointInfo {
                    address: 0x81,
                    attributes: 3,
                    max_packet_size: 64,
                    interval: 1,
                }],
            ),
            (
                1,
                vec![
                    EndpointInfo {
                        address: 0x82,
                        attributes: 2,
                        max_packet_size: 64,
                        interval: 0,
                    },
                    EndpointInfo {
                        address: 0x03,
                        attributes: 2,
                        max_packet_size: 64,
                        interval: 0,
                    },
                ],
            ),
        ]);
        // minimal IAD descriptor in raw config
        t.set_raw_descriptors(vec![
            9, 4, 0, 0, 1, 2, 2, 0, 0, 7, 5, 0x81, 3, 64, 0, 1, 9, 4, 1, 0, 2, 10, 0, 0, 7, 5,
            0x82, 2, 64, 0, 7, 5, 0x03, 2, 64, 0, 8, 11, 0, 2, 2, 2, 0, 0,
        ]);
        t
    }

    pub fn cdc_multi() -> Self {
        let t = Self::cdc_iad();
        t.set_interfaces(vec![
            InterfaceInfo {
                id: 0,
                class: 2,
                subclass: 2,
                protocol: 0,
            },
            InterfaceInfo {
                id: 1,
                class: 10,
                subclass: 0,
                protocol: 0,
            },
            InterfaceInfo {
                id: 2,
                class: 2,
                subclass: 2,
                protocol: 0,
            },
            InterfaceInfo {
                id: 3,
                class: 10,
                subclass: 0,
                protocol: 0,
            },
        ]);
        t.configure_endpoints(&[
            (
                1,
                vec![
                    EndpointInfo {
                        address: 0x82,
                        attributes: 2,
                        max_packet_size: 64,
                        interval: 0,
                    },
                    EndpointInfo {
                        address: 0x03,
                        attributes: 2,
                        max_packet_size: 64,
                        interval: 0,
                    },
                ],
            ),
            (
                3,
                vec![
                    EndpointInfo {
                        address: 0x84,
                        attributes: 2,
                        max_packet_size: 64,
                        interval: 0,
                    },
                    EndpointInfo {
                        address: 0x05,
                        attributes: 2,
                        max_packet_size: 64,
                        interval: 0,
                    },
                ],
            ),
        ]);
        t
    }

    pub fn gsm_modem() -> Self {
        let t = Self::cdc_single_iface();
        t.set_vendor_product(0x1782, 0x4D10);
        t.set_interfaces(vec![InterfaceInfo {
            id: 0,
            class: 255,
            subclass: 0,
            protocol: 0,
        }]);
        t.configure_endpoints(&[(
            0,
            vec![
                EndpointInfo {
                    address: 0x81,
                    attributes: 2,
                    max_packet_size: 64,
                    interval: 0,
                },
                EndpointInfo {
                    address: 0x02,
                    attributes: 2,
                    max_packet_size: 64,
                    interval: 0,
                },
            ],
        )]);
        t
    }

    pub fn chrome_ccd_3port() -> Self {
        let t = Self::cdc_single_iface();
        t.set_vendor_product(0x18D1, 0x5014);
        let mut ifaces = Vec::new();
        let mut eps = Vec::new();
        for n in 0..3u8 {
            ifaces.push(InterfaceInfo {
                id: n,
                class: 255,
                subclass: 0,
                protocol: 0,
            });
            eps.push((
                n,
                vec![
                    EndpointInfo {
                        address: 0x81 + n * 2,
                        attributes: 2,
                        max_packet_size: 64,
                        interval: 0,
                    },
                    EndpointInfo {
                        address: 0x02 + n * 2,
                        attributes: 2,
                        max_packet_size: 64,
                        interval: 0,
                    },
                ],
            ));
        }
        t.set_interfaces(ifaces);
        t.configure_endpoints(&eps);
        t
    }

    fn also<F: FnOnce(&Self)>(self, f: F) -> Self {
        f(&self);
        self
    }

    pub fn recorded_bulk_out(&self) -> Vec<RecordedBulkOut> {
        self.inner.lock().unwrap().recorded_bulk_out.clone()
    }

    pub fn push_rx(&self, data: &[u8]) {
        let mut s = self.inner.lock().unwrap();
        s.rx_queue.extend(data);
    }

    pub fn push_interrupt_in(&self, data: &[u8]) {
        let mut s = self.inner.lock().unwrap();
        s.interrupt_in_queue.extend(data);
    }

    /// Drain and return bytes written to bulk OUT (test asserts).
    pub fn take_tx(&self) -> Vec<u8> {
        let mut s = self.inner.lock().unwrap();
        std::mem::take(&mut s.tx_log)
    }

    /// Snapshot recorded control transfers.
    pub fn recorded_controls(&self) -> Vec<RecordedControl> {
        self.inner.lock().unwrap().recorded.clone()
    }

    pub fn clear_recorded(&self) {
        let mut s = self.inner.lock().unwrap();
        s.recorded.clear();
        s.recorded_bulk_out.clear();
    }

    pub fn claimed_interfaces(&self) -> Vec<u8> {
        self.inner.lock().unwrap().claimed.clone()
    }

    /// Queue the next `control_in` response payload (FIFO).
    pub fn script_control_in_response(&self, data: Vec<u8>) {
        self.inner.lock().unwrap().control_in_responses.push(data);
    }

    pub fn inject_bulk_read_error(&self, msg: impl Into<String>) {
        self.inner.lock().unwrap().bulk_read_error = Some(msg.into());
    }

    pub fn set_interfaces(&self, interfaces: Vec<InterfaceInfo>) {
        self.inner.lock().unwrap().interfaces = interfaces;
    }

    pub fn configure_endpoints(&self, layout: &[(u8, Vec<EndpointInfo>)]) {
        let mut s = self.inner.lock().unwrap();
        s.endpoints = layout
            .iter()
            .flat_map(|(iface, eps)| eps.iter().map(move |ep| (*iface, *ep)))
            .collect();
    }

    pub fn set_raw_descriptors(&self, raw: Vec<u8>) {
        self.inner.lock().unwrap().raw_descriptors = raw;
    }

    pub fn set_vendor_product(&self, vendor_id: u16, product_id: u16) {
        let mut s = self.inner.lock().unwrap();
        s.device_descriptor[8] = (vendor_id & 0xff) as u8;
        s.device_descriptor[9] = (vendor_id >> 8) as u8;
        s.device_descriptor[10] = (product_id & 0xff) as u8;
        s.device_descriptor[11] = (product_id >> 8) as u8;
    }

    pub fn patch_device_descriptor(&self, mut patch: impl FnMut(&mut [u8; 18])) {
        let mut s = self.inner.lock().unwrap();
        patch(&mut s.device_descriptor);
    }
}

struct FakeBulkIn {
    inner: Arc<Mutex<FakeState>>,
    interrupt: bool,
    endpoint: u8,
}

impl Drop for FakeBulkIn {
    fn drop(&mut self) {
        if let Ok(mut s) = self.inner.lock() {
            s.open_endpoints.retain(|&e| e != self.endpoint);
        }
    }
}

impl BulkIn for FakeBulkIn {
    fn read(&mut self, buf: &mut [u8], _timeout_ms: u32) -> Result<ReadOutcome> {
        let mut s = self.inner.lock().unwrap();
        if let Some(err) = s.bulk_read_error.take() {
            return Err(UsbSerialError::Io(err));
        }
        let queue = if self.interrupt {
            &mut s.interrupt_in_queue
        } else {
            &mut s.rx_queue
        };
        if queue.is_empty() {
            return Ok(ReadOutcome::TimedOut);
        }
        let n = buf.len().min(queue.len());
        for (i, byte) in queue.drain(..n).enumerate() {
            buf[i] = byte;
        }
        Ok(ReadOutcome::Data(buf[..n].to_vec()))
    }

    fn cancel_all(&mut self) {}

    fn clear_halt(&mut self) -> Result<()> {
        Ok(())
    }
}

struct FakeBulkOut {
    inner: Arc<Mutex<FakeState>>,
    endpoint: u8,
}

impl Drop for FakeBulkOut {
    fn drop(&mut self) {
        if let Ok(mut s) = self.inner.lock() {
            s.open_endpoints.retain(|&e| e != self.endpoint);
        }
    }
}

impl BulkOut for FakeBulkOut {
    fn write(&mut self, data: &[u8], _timeout_ms: u32) -> Result<usize> {
        let mut s = self.inner.lock().unwrap();
        s.tx_log.extend_from_slice(data);
        s.recorded_bulk_out.push(RecordedBulkOut {
            endpoint: self.endpoint,
            data: data.to_vec(),
        });
        Ok(data.len())
    }

    fn clear_halt(&mut self) -> Result<()> {
        Ok(())
    }
}

impl Transport for FakeTransport {
    fn raw_device_descriptor(&self) -> [u8; 18] {
        self.inner.lock().unwrap().device_descriptor
    }

    fn raw_descriptors(&self) -> Vec<u8> {
        self.inner.lock().unwrap().raw_descriptors.clone()
    }

    fn device_class(&self) -> u8 {
        self.raw_device_descriptor()[4]
    }

    fn interfaces(&self) -> Vec<InterfaceInfo> {
        self.inner.lock().unwrap().interfaces.clone()
    }

    fn endpoints(&self, interface: u8) -> Vec<EndpointInfo> {
        self.inner
            .lock()
            .unwrap()
            .endpoints
            .iter()
            .filter(|(iface, _)| *iface == interface)
            .map(|(_, ep)| *ep)
            .collect()
    }

    fn claim_interface(&self, interface: u8) -> Result<()> {
        self.inner.lock().unwrap().claimed.push(interface);
        Ok(())
    }

    fn release_interface(&self, interface: u8) -> Result<()> {
        let mut s = self.inner.lock().unwrap();
        s.claimed.retain(|&i| i != interface);
        Ok(())
    }

    fn control_out(&self, req: &ControlRequest) -> Result<usize> {
        let mut s = self.inner.lock().unwrap();
        s.recorded.push(RecordedControl {
            request_type: req.request_type,
            request: req.request,
            value: req.value,
            index: req.index,
            data: req.data.clone(),
        });
        Ok(req.data.len())
    }

    fn control_in(&self, req: &ControlRequest) -> Result<Vec<u8>> {
        let mut s = self.inner.lock().unwrap();
        s.recorded.push(RecordedControl {
            request_type: req.request_type,
            request: req.request,
            value: req.value,
            index: req.index,
            data: req.data.clone(),
        });
        if let Some(resp) = s.control_in_responses.pop() {
            return Ok(resp);
        }
        Ok(req.data.clone())
    }

    fn open_bulk_in(&self, endpoint: u8, _max_packet_size: u16) -> Result<Box<dyn BulkIn>> {
        self.mark_endpoint_open(endpoint)?;
        Ok(Box::new(FakeBulkIn {
            inner: self.inner.clone(),
            interrupt: false,
            endpoint,
        }))
    }

    fn open_bulk_out(&self, endpoint: u8, _max_packet_size: u16) -> Result<Box<dyn BulkOut>> {
        self.mark_endpoint_open(endpoint)?;
        Ok(Box::new(FakeBulkOut {
            inner: self.inner.clone(),
            endpoint,
        }))
    }

    fn open_interrupt_in(&self, endpoint: u8, _max_packet_size: u16) -> Result<Box<dyn BulkIn>> {
        self.mark_endpoint_open(endpoint)?;
        Ok(Box::new(FakeBulkIn {
            inner: self.inner.clone(),
            interrupt: true,
            endpoint,
        }))
    }
}

impl FakeTransport {
    fn mark_endpoint_open(&self, endpoint: u8) -> Result<()> {
        let mut s = self.inner.lock().unwrap();
        if s.open_endpoints.contains(&endpoint) {
            return Err(UsbSerialError::Io("endpoint already in use".into()));
        }
        s.open_endpoints.push(endpoint);
        Ok(())
    }
}
