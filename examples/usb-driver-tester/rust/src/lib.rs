//! JNI facade for standalone USB driver hardware self-tests.

use android_usb_serial::config::{DataBits, FlowControl, LineConfig, Parity, StopBits};
use android_usb_serial::fake::FakeTransport;
use android_usb_serial::probe::{DriverType, ProbeTable};
use android_usb_serial::transport::Transport;
use std::ffi::{c_char, CString};
use std::sync::Arc;

fn cstr(s: String) -> *mut c_char {
    CString::new(s).unwrap_or_default().into_raw()
}

fn driver_name(t: DriverType) -> &'static str {
    match t {
        DriverType::CdcAcm => "cdc_acm",
        DriverType::Cp21xx => "cp21xx",
        DriverType::Ftdi => "ftdi",
        DriverType::Prolific => "prolific",
        DriverType::Ch34x => "ch34x",
        DriverType::GsmModem => "gsm_modem",
        DriverType::ChromeCcd => "chrome_ccd",
    }
}

fn run_fake_matrix(driver: DriverType, fake: FakeTransport) -> Result<(), String> {
    for _ in 0..4 {
        fake.script_control_in_response(vec![0, 0]);
    }
    fake.script_control_in_response(vec![0]);
    let transport: Arc<dyn Transport> = Arc::new(fake);
    let mut port = android_usb_serial::device::open_port(transport, 0).map_err(|e| e.to_string())?;
    if port
        .set_line_config(LineConfig {
            baud_rate: 115_200,
            data_bits: DataBits::Eight,
            parity: Parity::None,
            stop_bits: StopBits::One,
        })
        .is_ok()
    {
        port.set_flow_control(FlowControl::None)
            .map_err(|e| e.to_string())?;
        let _ = port.set_dtr(true);
        let _ = port.set_rts(true);
        port.write(b"PING").map_err(|e| e.to_string())?;
    }
    port.close();
    let _ = driver;
    Ok(())
}

#[cfg(target_os = "android")]
fn run_real_fd_test(fd: i32, vendor_id: u16, product_id: u16) -> String {
    use android_usb_serial::device::open_port;
    use android_usb_serial::from_raw_fd;
    use android_usb_serial::NusbTransport;

    let mut lines = Vec::new();
    let table = ProbeTable::default_table();
    let driver = table.find(vendor_id, product_id, &[]);
    lines.push(format!("probe {} → {}", hex_vid_pid(vendor_id, product_id), driver_name(driver)));

    let device = match from_raw_fd(fd) {
        Ok(d) => d,
        Err(e) => {
            lines.push(format!("FAIL from_raw_fd: {e}"));
            return lines.join("\n");
        }
    };
    let transport = match NusbTransport::from_device(device) {
        Ok(t) => t,
        Err(e) => {
            lines.push(format!("FAIL NusbTransport: {e}"));
            return lines.join("\n");
        }
    };
    let ifaces = transport.interfaces();
    let resolved = table.find(vendor_id, product_id, &ifaces);
    lines.push(format!(
        "PASS descriptor ({} iface(s), driver={})",
        ifaces.len(),
        driver_name(resolved)
    ));

    let shared: Arc<dyn Transport> = Arc::new(transport);
    let mut port = match open_port(shared.clone(), 0) {
        Ok(p) => p,
        Err(e) => {
            lines.push(format!("FAIL open_port: {e}"));
            return lines.join("\n");
        }
    };
    lines.push("PASS open".to_string());

    if port
        .set_line_config(LineConfig {
            baud_rate: 115_200,
            data_bits: DataBits::Eight,
            parity: Parity::None,
            stop_bits: StopBits::One,
        })
        .is_ok()
    {
        let _ = port.set_dtr(true);
        let _ = port.set_rts(true);
        match port.write(b"AT\r\n") {
            Ok(n) => lines.push(format!("PASS write {n} bytes")),
            Err(e) => lines.push(format!("WARN write: {e}")),
        }
        let mut buf = [0u8; 64];
        match port.read(&mut buf) {
            Ok(n) if n > 0 => lines.push(format!("PASS read {n} bytes")),
            Ok(_) => lines.push("PASS read (no data yet)".to_string()),
            Err(e) => lines.push(format!("WARN read: {e}")),
        }
    } else {
        lines.push("SKIP line config (driver limited)".to_string());
    }

    port.close();
    lines.push("PASS close".to_string());
    lines.join("\n")
}

fn hex_vid_pid(v: u16, p: u16) -> String {
    format!("{v:04X}:{p:04X}")
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_serialport_usbtester_MainActivity_nativeProbeDriver(
    _env: *mut std::ffi::c_void,
    _class: *mut std::ffi::c_void,
    vendor_id: i32,
    product_id: i32,
) -> *mut c_char {
    let table = ProbeTable::default_table();
    let driver = table.find(vendor_id as u16, product_id as u16, &[]);
    cstr(driver_name(driver).to_string())
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_serialport_usbtester_MainActivity_nativeRunSelfTest(
    _env: *mut std::ffi::c_void,
    _class: *mut std::ffi::c_void,
) -> *mut c_char {
    let mut lines = Vec::new();
    let matrix: Vec<(DriverType, fn() -> FakeTransport)> = vec![
        (DriverType::Ftdi, FakeTransport::ftdi_ft232r),
        (DriverType::Cp21xx, FakeTransport::cp2102),
        (DriverType::Ch34x, FakeTransport::ch340_dual_iface),
        (DriverType::Prolific, FakeTransport::pl2303_hx),
        (DriverType::CdcAcm, FakeTransport::cdc_single_iface),
    ];
    for (driver, build) in matrix {
        let name = driver_name(driver);
        match run_fake_matrix(driver, build()) {
            Ok(()) => lines.push(format!("PASS {name}")),
            Err(e) => lines.push(format!("FAIL {name}: {e}")),
        }
    }
    lines.push(format!("version {}", env!("CARGO_PKG_VERSION")));
    cstr(lines.join("\n"))
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_serialport_usbtester_MainActivity_nativeOpenAndTest(
    _env: *mut std::ffi::c_void,
    _class: *mut std::ffi::c_void,
    fd: i32,
    vendor_id: i32,
    product_id: i32,
) -> *mut c_char {
    cstr(run_real_fd_test(fd, vendor_id as u16, product_id as u16))
}

#[cfg(not(target_os = "android"))]
fn main() {}
