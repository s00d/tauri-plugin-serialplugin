//! Stories on port records and CMUX that no IPC sequence produces on its own: a record
//! in `Closed` state, a stopped RX hub, a virtual port closing under load, and GSM 07.10
//! multiplexing against a fake modem.

use crate::common::*;
use tauri_plugin_serialplugin::mock_serial::MockSerialPort;
use tauri_plugin_serialplugin::state::SerialportInfo;

/// Given a port record left in `Closed` state
/// When the app writes to it
/// Then the write fails
#[test]
fn a_closed_record_refuses_writes() {
    let plugin = Plugin::new();
    let path = "/dev/story-closed-record";
    plugin.sp().serialports.lock().unwrap().insert(
        path.into(),
        SerialportInfo {
            state: PortState::Closed,
        },
    );
    assert!(plugin
        .invoke("write", json!({ "path": path, "value": "x" }))
        .is_err());
}

/// Given a managed port whose RX hub was stopped
/// When the app reads from it
/// Then the hub runs again
#[test]
fn a_stopped_hub_restarts_on_the_next_read() {
    let plugin = Plugin::new();
    let sp = plugin.sp();
    let path = "/dev/story-hub-restart".to_string();
    sp.insert_port(path.clone(), Box::new(MockSerialPort::new()))
        .unwrap();
    let hub_running = || {
        let ports = sp.serialports.lock().unwrap();
        match ports.get(&path).map(|i| &i.state) {
            Some(PortState::Connected(cp)) => cp
                .rx_hub
                .lock()
                .unwrap()
                .as_ref()
                .is_some_and(|h| !h.is_finished()),
            _ => false,
        }
    };
    {
        let ports = sp.serialports.lock().unwrap();
        let Some(PortState::Connected(cp)) = ports.get(&path).map(|i| &i.state) else {
            panic!("connected");
        };
        let hub = cp.rx_hub.lock().unwrap().take();
        if let Some(hub) = hub {
            match Arc::try_unwrap(hub) {
                Ok(hub) => hub.stop(),
                Err(hub) => hub.request_stop(),
            }
        }
    }
    assert!(!hub_running());
    let _ = plugin.invoke("read", json!({ "path": path, "timeout": 10, "size": 8 }));
    assert!(hub_running(), "hub restarted");
}

/// Given a CMUX virtual port record next to its physical port
/// When the app closes the virtual port while another caller lists managed ports
/// Then the close completes (no lock-order deadlock)
#[test]
fn closing_a_virtual_port_under_managed_ports_completes() {
    use tauri_plugin_serialplugin::state::VirtualPortRef;
    let plugin = Plugin::new();
    let sp = plugin.sp();
    let physical = "/dev/story-physical".to_string();
    let virtual_path = format!("{physical}#dlci=2");
    sp.insert_port(physical.clone(), Box::new(MockSerialPort::new()))
        .unwrap();
    sp.virtual_ports.lock().unwrap().insert(
        virtual_path.clone(),
        VirtualPortRef {
            physical_path: physical,
            dlci: 2,
            exchange_cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            tx_queue: Arc::new(tauri_plugin_serialplugin::port::tx_queue::PortTxQueue::new()),
        },
    );
    let lister = {
        let webview = plugin.webview.clone();
        let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = done.clone();
        (
            std::thread::spawn(move || {
                while !flag.load(Ordering::SeqCst) {
                    invoke(&webview, "managed_ports", json!({})).expect("managed_ports");
                }
            }),
            done,
        )
    };
    let _ = plugin.invoke("close", json!({ "path": virtual_path }));
    lister.1.store(true, Ordering::SeqCst);
    lister.0.join().unwrap();
    assert!(!plugin.managed().contains(&virtual_path));
}

/// Given a CMUX session with an exchange waiting on DLCI 2
/// When a UIH frame for DLCI 2 carrying `OK` arrives on the physical port
/// Then that exchange completes with OK
#[cfg(unix)]
#[test]
fn a_uih_frame_completes_its_dlci_exchange() {
    use tauri_plugin_serialplugin::cmux::{encode_uih, CmuxSession};
    use tauri_plugin_serialplugin::events::{
        AtResultFormat, ExchangeCompletionMode, RxPrepareMode,
    };
    use tauri_plugin_serialplugin::exchange::options::ResolvedExchangeOptions;
    use tauri_plugin_serialplugin::hub::ExchangeWaiter;

    let (_master, port, _) = pty();
    let path = "story-cmux-session".to_string();
    let session = CmuxSession::with_n1(
        path.clone(),
        Arc::new(tauri_plugin_serialplugin::cmux::SerialPortIo(Arc::new(
            Mutex::new(Box::new(port) as Box<dyn serialport::SerialPort>),
        ))),
        31,
    );
    session.register_dlci(2, format!("{path}#dlci=2"));
    let options = ResolvedExchangeOptions {
        timeout_ms: 3000,
        max_bytes: 4096,
        terminators: vec![],
        idle_ms: None,
        rx_prepare: RxPrepareMode::None,
        drain_idle_ms: 50,
        drain_max_ms: 200,
        completion_mode: ExchangeCompletionMode::AtFinalLine,
        result_format: AtResultFormat::Verbose,
        command: Some("AT".into()),
        solicited_prefixes: vec![],
    };
    let waiter = ExchangeWaiter::new(options, Arc::new(std::sync::atomic::AtomicBool::new(false)));
    session.set_exchange_waiter(2, waiter.clone());
    session.feed_physical_rx(&encode_uih(2, b"AT\r\r\nOK\r\n"));
    let (raw, matched) = waiter.wait(1000).expect("virtual exchange complete");
    assert!(String::from_utf8_lossy(&raw).contains("OK"));
    assert!(matches!(
        matched,
        tauri_plugin_serialplugin::at::parse::ExchangeMatch::Ok
    ));
}

/// Given a fake 27.010 modem on a PTY (OK to `AT+CMUX=0`, UA to every SABM, OK to AT)
/// When the app enables CMUX, opens DLCI 0 and DLCI 2, sends `at` on DLCI 2 and closes
/// the physical port
/// Then DLCI 0 is refused, the AT on the virtual port is OK, SABMs went out control
/// channel first, nothing stays managed, and closing sent CLD
#[cfg(unix)]
#[test]
fn cmux_virtual_port_round_trip() {
    use tauri_plugin_serialplugin::cmux::frame::{encode_frame, Deframer, PF, SABM, UA, UIH};
    let plugin = Plugin::new();
    let (mut master, path) = pty_port(&plugin);
    let (cld_tx, cld_rx) = mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        let mut buf = [0u8; 1024];
        let mut text = Vec::new();
        let mut muxed = false;
        let mut deframer = Deframer::default();
        let mut opened = Vec::new();
        loop {
            let n = match master.read(&mut buf) {
                Ok(n) => n,
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut => continue,
                Err(_) => return,
            };
            if !muxed {
                text.extend_from_slice(&buf[..n]);
                if text.ends_with(b"AT+CMUX=0\r") {
                    master.write_all(b"\r\nOK\r\n").unwrap();
                    muxed = true;
                }
                continue;
            }
            for f in deframer.feed(&buf[..n]) {
                match f.kind {
                    SABM => {
                        opened.push(f.dlci);
                        master
                            .write_all(&encode_frame(f.dlci, true, UA | PF, &[]))
                            .unwrap();
                    }
                    UIH if f.dlci == 0 && f.payload.first() == Some(&0xC3) => {
                        let _ = cld_tx.send(opened.clone());
                        return;
                    }
                    UIH if f.payload.starts_with(b"AT") => {
                        master
                            .write_all(&encode_frame(f.dlci, false, UIH, b"\r\nOK\r\n"))
                            .unwrap();
                    }
                    _ => {}
                }
            }
        }
    });
    plugin
        .invoke(
            "enable_mux",
            json!({ "path": path, "options": { "command": "AT+CMUX=0", "timeoutMs": 2000 } }),
        )
        .expect("enable_mux");
    assert!(
        plugin
            .invoke("open_mux_channel", json!({ "path": path, "dlci": 0 }))
            .is_err(),
        "DLCI 0 is the control channel"
    );
    let virtual_path = plugin
        .invoke("open_mux_channel", json!({ "path": path, "dlci": 2 }))
        .expect("open_mux_channel");
    let res = plugin
        .invoke(
            "at",
            json!({ "path": virtual_path, "command": "AT", "options": null }),
        )
        .expect("virtual AT");
    assert_eq!(res["status"], "ok");
    plugin
        .invoke("close", json!({ "path": path }))
        .expect("close");
    assert!(plugin.managed().is_empty());
    let opened = cld_rx.recv_timeout(WAIT).expect("CLD on close");
    assert_eq!(opened, vec![0, 2], "SABM order: control channel first");
}

/// Given a `tcp://` port opened over IPC with a read waiting for data (60 s timeout)
/// When a second read is sent while the first is pending
/// Then the second fails as Busy and the first keeps waiting
/// When the device then goes away (the peer closes)
/// Then the pending read fails as Disconnected without waiting for its timeout
#[test]
fn a_pending_read_refuses_a_second_and_fails_on_disconnect() {
    use tauri_plugin_serialplugin::error::ErrorKind;
    let plugin = Plugin::new();
    let (listener, path) = tcp_listener();
    open_ipc(&plugin, &path).expect("open");
    let peer = accept(&listener);
    let kind = |r: Result<Value, Value>| {
        r.map(|_| ())
            .map_err(|e| ErrorKind::from_message(e.as_str().unwrap()))
    };
    let start_read = || {
        let (webview, path) = (plugin.webview.clone(), path.clone());
        std::thread::spawn(move || {
            invoke(
                &webview,
                "read_binary",
                json!({ "path": path, "timeout": 60000 }),
            )
        })
    };
    // Which read holds the slot is not observable: probe with short reads until one is
    // refused. A probe that held the slot itself refuses the first read: start it again.
    let mut first = start_read();
    eventually("a second read refused as Busy", || {
        let probe = kind(plugin.invoke("read_binary", json!({ "path": path, "timeout": 1 })));
        if probe == Err(ErrorKind::Busy) {
            return true;
        }
        if first.is_finished() {
            let r = std::mem::replace(&mut first, start_read()).join().unwrap();
            assert_eq!(
                kind(r),
                Err(ErrorKind::Busy),
                "first read refused by a probe"
            );
        }
        false
    });
    assert!(!first.is_finished(), "the first read keeps waiting");
    drop(peer);
    let deadline = Instant::now() + Duration::from_secs(10);
    while !first.is_finished() {
        assert!(Instant::now() < deadline, "pending read never failed");
        std::thread::yield_now();
    }
    assert_eq!(kind(first.join().unwrap()), Err(ErrorKind::Disconnected));
}
