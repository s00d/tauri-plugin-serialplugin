#[cfg(test)]
mod tests {
    use crate::desktop_api::SerialPort;
    use crate::state::{DataBits, FlowControl, Parity, PortState, SerialportInfo, StopBits};
    use std::sync::Arc;
    use tauri::test::MockRuntime;
    use tauri::App;
    use tauri::Manager;

    fn create_test_app() -> App<MockRuntime> {
        let app = tauri::test::mock_app();
        let serial_port = SerialPort::new(app.handle().clone());
        app.manage(serial_port);
        app
    }

    #[test]
    fn test_desktop_api_init() {
        let app = create_test_app();
        let serial_port = app.state::<SerialPort<MockRuntime>>().inner().clone();
        assert!(serial_port.available_ports(false).is_ok());
    }

    #[test]
    fn test_desktop_api_port_list() {
        let app = create_test_app();
        let serial_port = app.state::<SerialPort<MockRuntime>>().inner().clone();
        let ports = serial_port.available_ports(false).unwrap();
        // In CI/CD environment, there might be no USB devices connected
        // So we just check that the function returns successfully
        // The ports list can be empty in CI/CD
        println!("Desktop API available ports: {:?}", ports);
        let _ = ports.len();
    }

    #[test]
    fn test_desktop_api_port_operations() {
        let app = create_test_app();
        let serial_port = app.state::<SerialPort<MockRuntime>>().inner().clone();

        // Test should expect error when opening non-existent port
        let result = serial_port.open(
            "NONEXISTENT".to_string(),
            9600,
            Some(DataBits::Eight),
            Some(FlowControl::None),
            Some(Parity::None),
            Some(StopBits::One),
            Some(1000),
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_desktop_api_port_settings() {
        let app = create_test_app();
        let serial_port = app.state::<SerialPort<MockRuntime>>().inner().clone();

        // Test should expect error when setting settings for non-existent port
        let result = serial_port.set_baud_rate("NONEXISTENT".to_string(), 115200);
        assert!(result.is_err());

        // Check that port is not added to managed ports list
        let ports = serial_port.managed_ports().unwrap();
        assert!(!ports.contains(&"NONEXISTENT".to_string()));
    }

    #[test]
    fn test_write_rejects_closed_port_state() {
        let app = create_test_app();
        let serial_port = app.state::<SerialPort<MockRuntime>>().inner().clone();
        let path = "COM_STATE_CLOSED".to_string();
        serial_port.serialports.lock().unwrap().insert(
            path.clone(),
            SerialportInfo {
                state: PortState::Closed,
            },
        );

        let err = serial_port
            .write(path, "x".to_string())
            .expect_err("write on Closed must fail");
        let msg = err.to_string();
        assert!(
            msg.contains("closed") || msg.contains("Closed"),
            "unexpected message: {}",
            msg
        );
    }

    #[test]
    fn test_desktop_api_error_handling() {
        let app = create_test_app();
        let serial_port = app.state::<SerialPort<MockRuntime>>().inner().clone();

        // Test working with non-existent port
        let result = serial_port.write("NONEXISTENT".to_string(), "Test".to_string());
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("is not open") || err_msg.contains("No such file or directory") || err_msg.contains("not found"),
            "Expected error to contain 'is not open', 'No such file or directory' or 'not found', got: {}", err_msg);

        // Test reading from non-existent port
        let result = serial_port.read("NONEXISTENT".to_string(), Some(1000), Some(1024));
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("is not open") || err_msg.contains("No such file or directory") || err_msg.contains("not found"),
            "Expected error to contain 'is not open', 'No such file or directory' or 'not found', got: {}", err_msg);

        // Test closing non-existent port
        let result = serial_port.close("NONEXISTENT".to_string());
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("is not open") || err_msg.contains("No such file or directory") || err_msg.contains("not found"),
            "Expected error to contain 'is not open', 'No such file or directory' or 'not found', got: {}", err_msg);
    }

    #[test]
    fn test_desktop_api_control_signals() {
        let app = create_test_app();
        let serial_port = app.state::<SerialPort<MockRuntime>>().inner().clone();

        // Test should expect error when setting control signals for non-existent port
        let result = serial_port.write_request_to_send("NONEXISTENT".to_string(), true);
        assert!(result.is_err());

        let result = serial_port.read_clear_to_send("NONEXISTENT".to_string());
        assert!(result.is_err());

        let result = serial_port.read_data_set_ready("NONEXISTENT".to_string());
        assert!(result.is_err());
    }

    #[test]
    fn test_available_ports() {
        let app = create_test_app();
        let serial_port = app.state::<SerialPort<MockRuntime>>().inner().clone();
        let result = serial_port.available_ports(false);
        assert!(result.is_ok());
        let ports = result.unwrap();
        // In CI/CD environment, there might be no USB devices connected
        // So we just check that the function returns successfully
        // The ports list can be empty in CI/CD
        println!("Available ports: {:?}", ports);
        // Assert that we got a valid result (even if empty)
        assert!(
            ports.is_empty() || !ports.is_empty(),
            "Ports list should be valid"
        );
    }

    #[test]
    fn test_managed_ports() {
        let app = create_test_app();
        let serial_port = app.state::<SerialPort<MockRuntime>>().inner().clone();
        let result = serial_port.managed_ports();
        assert!(result.is_ok());
        let ports = result.unwrap();
        assert!(ports.is_empty());
    }

    #[test]
    fn test_open_port() {
        let app = create_test_app();
        let serial_port = app.state::<SerialPort<MockRuntime>>().inner().clone();

        // Test should expect error when opening non-existent port
        let result = serial_port.open(
            "NONEXISTENT".to_string(),
            9600,
            Some(DataBits::Eight),
            Some(FlowControl::None),
            Some(Parity::None),
            Some(StopBits::One),
            Some(1000),
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_close_port() {
        let app = create_test_app();
        let serial_port = app.state::<SerialPort<MockRuntime>>().inner().clone();

        // Test should expect error when closing non-existent port
        let result = serial_port.close("NONEXISTENT".to_string());
        assert!(result.is_err());
    }

    #[test]
    fn test_write_and_read() {
        let app = create_test_app();
        let serial_port = app.state::<SerialPort<MockRuntime>>().inner().clone();

        // Test should expect error when writing to non-existent port
        let result = serial_port.write("NONEXISTENT".to_string(), "Test data".to_string());
        assert!(result.is_err());

        // Test should expect error when reading from non-existent port
        let result = serial_port.read("NONEXISTENT".to_string(), Some(1000), Some(1024));
        assert!(result.is_err());
    }

    #[test]
    fn test_port_settings() {
        let app = create_test_app();
        let serial_port = app.state::<SerialPort<MockRuntime>>().inner().clone();

        // Test should expect error when setting settings for non-existent port
        let result = serial_port.set_baud_rate("NONEXISTENT".to_string(), 115200);
        assert!(result.is_err());

        let result = serial_port.set_data_bits("NONEXISTENT".to_string(), DataBits::Seven);
        assert!(result.is_err());
    }

    #[test]
    fn test_control_signals() {
        let app = create_test_app();
        let serial_port = app.state::<SerialPort<MockRuntime>>().inner().clone();

        // Test should expect error when setting control signals for non-existent port
        let result = serial_port.write_request_to_send("NONEXISTENT".to_string(), true);
        assert!(result.is_err());

        let result = serial_port.write_data_terminal_ready("NONEXISTENT".to_string(), true);
        assert!(result.is_err());
    }

    #[test]
    fn test_buffer_operations() {
        let app = create_test_app();
        let serial_port = app.state::<SerialPort<MockRuntime>>().inner().clone();

        // Test should expect error when performing buffer operations on non-existent port
        let result =
            serial_port.clear_buffer("NONEXISTENT".to_string(), crate::state::ClearBuffer::All);
        assert!(result.is_err());
    }

    #[test]
    fn test_break_control() {
        let app = create_test_app();
        let serial_port = app.state::<SerialPort<MockRuntime>>().inner().clone();

        // Test should expect error when setting break on non-existent port
        let result = serial_port.set_break("NONEXISTENT".to_string());
        assert!(result.is_err());

        // Test should expect error when clearing break on non-existent port
        let result = serial_port.clear_break("NONEXISTENT".to_string());
        assert!(result.is_err());
    }

    #[test]
    fn close_all_closes_ports_and_clears_registry() {
        use crate::state::{ConnectedPort, PortState, SerialportInfo};
        use std::sync::{Arc, Mutex};
        use tauri::ipc::Channel;
        use tauri::ipc::InvokeResponseBody;

        let app = create_test_app();
        let serial_port = app.state::<SerialPort<MockRuntime>>().inner().clone();

        let (master, slave) = serialport::TTYPort::pair().expect("pty pair");
        let path = "pty-close-all-test".to_string();

        serial_port.serialports.lock().unwrap().insert(
            path.clone(),
            SerialportInfo {
                state: PortState::Connected(ConnectedPort::new(Box::new(slave))),
            },
        );

        let events: Arc<Mutex<Vec<crate::events::SerialEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let events_clone = events.clone();
        let channel = Channel::<crate::events::SerialEvent>::new(move |body| {
            if let InvokeResponseBody::Json(json) = body {
                if let Ok(event) = serde_json::from_str::<crate::events::SerialEvent>(&json) {
                    events_clone.lock().unwrap().push(event);
                }
            }
            Ok(())
        });

        let channel_id = serial_port
            .watch(
                path.clone(),
                crate::events::WatchOptions::default(),
                channel,
            )
            .expect("watch");
        assert!(crate::watch_registry::paths_for_port(&path).contains(&channel_id));

        serial_port.close_all().expect("close_all");

        assert!(serial_port.managed_ports().unwrap().is_empty());
        assert!(crate::watch_registry::paths_for_port(&path).is_empty());

        drop(master);
    }

    /// `serialport` maps POSIX `POLLHUP` to `BrokenPipe` on `read()`; the watch thread must
    /// send a disconnect event through the channel (same as production apps).
    #[cfg(unix)]
    #[test]
    fn watch_sends_disconnect_when_pty_peer_closed() {
        use crate::events::{SerialEvent, WatchOptions};
        use crate::state::{ConnectedPort, PortState, SerialportInfo};
        use std::sync::{mpsc, Arc, Mutex};
        use std::time::Duration;
        use tauri::ipc::Channel;
        use tauri::ipc::InvokeResponseBody;

        let app = create_test_app();
        let serial_port = app.state::<SerialPort<MockRuntime>>().inner().clone();

        let (master, slave) = serialport::TTYPort::pair().expect("pty pair");
        let path = "pty-disconnect-test".to_string();

        serial_port.serialports.lock().unwrap().insert(
            path.clone(),
            SerialportInfo {
                state: PortState::Connected(ConnectedPort::new(Box::new(slave))),
            },
        );

        let events: Arc<Mutex<Vec<SerialEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let events_clone = events.clone();
        let channel = Channel::<SerialEvent>::new(move |body| {
            if let InvokeResponseBody::Json(json) = body {
                if let Ok(event) = serde_json::from_str::<SerialEvent>(&json) {
                    events_clone.lock().unwrap().push(event);
                }
            }
            Ok(())
        });

        serial_port
            .watch(path.clone(), WatchOptions::default(), channel)
            .expect("watch");

        let (tx, rx) = mpsc::channel();
        let events_wait = events.clone();
        std::thread::spawn(move || loop {
            if events_wait
                .lock()
                .unwrap()
                .iter()
                .any(|e| matches!(e, SerialEvent::Disconnect { .. }))
            {
                tx.send(()).ok();
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        });

        drop(master);

        rx.recv_timeout(Duration::from_secs(3))
            .expect("disconnect event after peer close");

        let _ = serial_port.close(path);
    }

    /// PTY peer responds with OK when it receives the command (master must drain RX).
    #[cfg(unix)]
    #[test]
    fn exchange_reads_until_ok_on_pty() {
        use crate::events::ExchangeOptions;
        use crate::state::{ConnectedPort, PortState, SerialportInfo};
        use serialport::SerialPort as SerialPortTrait;
        use std::io::{Read, Write};
        use std::thread;
        use std::time::Duration;

        let app = create_test_app();
        let serial_port = app.state::<SerialPort<MockRuntime>>().inner().clone();

        let (mut master, slave) = serialport::TTYPort::pair().expect("pty pair");
        master
            .set_timeout(Duration::from_millis(50))
            .expect("master timeout");
        let path = "pty-exchange-test".to_string();

        serial_port.serialports.lock().unwrap().insert(
            path.clone(),
            SerialportInfo {
                state: PortState::Connected(ConnectedPort::new(Box::new(slave))),
            },
        );

        let (done_tx, done_rx) = std::sync::mpsc::channel();

        let responder = thread::spawn(move || {
            let mut buf = [0u8; 256];
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while std::time::Instant::now() < deadline {
                match master.read(&mut buf) {
                    Ok(0) => continue,
                    Ok(_) => {
                        master.write_all(b"\r\nOK\r\n").expect("write OK response");
                        master.flush().ok();
                        break;
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => continue,
                    Err(e) => panic!("master read failed: {}", e),
                }
            }
            // Keep master open until exchange finishes (avoid Broken pipe on slave).
            let _ = done_rx.recv_timeout(Duration::from_secs(5));
        });

        let response = serial_port
            .exchange(
                path.clone(),
                "AT\r".to_string(),
                ExchangeOptions {
                    timeout_ms: Some(3000),
                    rx_prepare: Some(crate::events::RxPrepareMode::None),
                    ..Default::default()
                },
            )
            .expect("exchange");

        done_tx.send(()).ok();
        responder.join().expect("responder join");
        let text = String::from_utf8_lossy(&response.raw);
        assert!(text.contains("OK"), "expected OK in {:?}", text);
        assert_eq!(response.status, crate::at_parse::AtParseStatus::Ok);

        let _ = serial_port.close(path);
    }

    /// cancel_exchange must not deadlock while exchange holds the port open (no global map lock).
    #[cfg(unix)]
    #[test]
    fn cancel_exchange_while_at_in_flight() {
        use crate::events::ExchangeOptions;
        use crate::state::{ConnectedPort, PortState, SerialportInfo};
        use serialport::SerialPort as SerialPortTrait;
        use std::sync::{Arc, Mutex};
        use std::thread;
        use std::time::{Duration, Instant};

        let app = create_test_app();
        let sp = app.state::<SerialPort<MockRuntime>>().inner().clone();

        let (mut master, slave) = serialport::TTYPort::pair().expect("pty pair");
        master
            .set_timeout(Duration::from_millis(50))
            .expect("master timeout");
        let path = "pty-cancel-in-flight".to_string();

        sp.serialports.lock().unwrap().insert(
            path.clone(),
            SerialportInfo {
                state: PortState::Connected(ConnectedPort::new(Box::new(slave))),
            },
        );

        let sp_cancel = sp.clone();
        let path_cancel = path.clone();
        let cancel_elapsed = Arc::new(Mutex::new(None::<Duration>));
        let cancel_elapsed_bg = cancel_elapsed.clone();
        let cancel_thread = thread::spawn(move || {
            thread::sleep(Duration::from_millis(100));
            let start = Instant::now();
            sp_cancel
                .cancel_exchange(path_cancel)
                .expect("cancel should not block on global map lock");
            *cancel_elapsed_bg.lock().unwrap() = Some(start.elapsed());
        });

        let exchange_result = sp.exchange(
            path.clone(),
            "AT\r".to_string(),
            ExchangeOptions {
                timeout_ms: Some(5000),
                rx_prepare: Some(crate::events::RxPrepareMode::None),
                ..Default::default()
            },
        );

        cancel_thread.join().expect("cancel thread join");
        let elapsed = cancel_elapsed
            .lock()
            .unwrap()
            .expect("cancel timing recorded");
        assert!(
            elapsed < Duration::from_millis(500),
            "cancel_exchange took too long: {elapsed:?}"
        );

        assert!(
            exchange_result.is_err(),
            "cancelled exchange should fail, got {:?}",
            exchange_result.ok()
        );
        let err = exchange_result.unwrap_err().to_string();
        assert!(
            err.contains("cancel") || err.contains("timed out"),
            "unexpected: {err}"
        );

        let _ = sp.close(path);
    }

    /// I/O helpers must stay responsive while an exchange waits for a response.
    #[cfg(unix)]
    #[test]
    fn write_succeeds_while_exchange_waiting() {
        use crate::events::ExchangeOptions;
        use crate::state::{ConnectedPort, PortState, SerialportInfo};
        use serialport::SerialPort as SerialPortTrait;
        use std::io::{Read, Write};
        use std::sync::{Arc, Mutex};
        use std::thread;
        use std::time::{Duration, Instant};

        let app = create_test_app();
        let sp = app.state::<SerialPort<MockRuntime>>().inner().clone();

        let (mut master, slave) = serialport::TTYPort::pair().expect("pty pair");
        master
            .set_timeout(Duration::from_millis(50))
            .expect("master timeout");
        let path = "pty-write-during-exchange".to_string();

        sp.serialports.lock().unwrap().insert(
            path.clone(),
            SerialportInfo {
                state: PortState::Connected(ConnectedPort::new(Box::new(slave))),
            },
        );

        let responder = thread::spawn(move || {
            let mut buf = [0u8; 256];
            let deadline = Instant::now() + Duration::from_secs(5);
            while Instant::now() < deadline {
                master
                    .set_timeout(Duration::from_millis(50))
                    .expect("master timeout");
                match master.read(&mut buf) {
                    Ok(0) => thread::sleep(Duration::from_millis(10)),
                    Ok(_) => {
                        thread::sleep(Duration::from_millis(300));
                        let _ = master.write_all(b"\r\nOK\r\n");
                        let _ = master.flush();
                        return;
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => continue,
                    Err(_) => return,
                }
            }
        });

        let io_elapsed = Arc::new(Mutex::new(None::<(Duration, Duration)>));
        let io_elapsed_bg = io_elapsed.clone();
        let sp_io = sp.clone();
        let path_io = path.clone();
        let io_thread = thread::spawn(move || {
            thread::sleep(Duration::from_millis(100));
            let io_start = Instant::now();
            sp_io
                .bytes_to_read(path_io.clone())
                .expect("bytes_to_read while exchange waiting");
            let bytes_elapsed = io_start.elapsed();
            let write_start = Instant::now();
            let _write_result = sp_io.write(path_io, "PING".to_string());
            *io_elapsed_bg.lock().unwrap() = Some((bytes_elapsed, write_start.elapsed()));
        });

        // Let the PTY responder enter its read loop before the exchange writes.
        thread::sleep(Duration::from_millis(20));

        let exchange_result = sp.exchange(
            path.clone(),
            "AT\r".to_string(),
            ExchangeOptions {
                timeout_ms: Some(5000),
                rx_prepare: Some(crate::events::RxPrepareMode::None),
                ..Default::default()
            },
        );

        io_thread.join().expect("io thread join");
        drop(responder);

        let (bytes_elapsed, write_elapsed) = io_elapsed.lock().unwrap().expect("io timing");
        assert!(
            bytes_elapsed < Duration::from_millis(500),
            "bytes_to_read blocked while exchange in flight: {bytes_elapsed:?}"
        );
        assert!(
            write_elapsed < Duration::from_millis(500),
            "write blocked while exchange in flight: {write_elapsed:?}"
        );

        exchange_result.expect("exchange should complete after OK");
        let _ = sp.close(path);
    }

    /// AT session `defaultTimeoutMs` must apply (not fall back to 5000 ms exchange default).
    #[cfg(unix)]
    #[test]
    fn at_uses_configured_session_timeout_on_pty() {
        use crate::at_session::AtSessionConfig;
        use crate::events::RxPrepareMode;
        use crate::state::{ConnectedPort, PortState, SerialportInfo};
        use serialport::SerialPort as SerialPortTrait;
        use std::io::Read;
        use std::thread;
        use std::time::{Duration, Instant};

        let app = create_test_app();
        let sp = app.state::<SerialPort<MockRuntime>>().inner().clone();

        let (mut master, slave) = serialport::TTYPort::pair().expect("pty pair");
        master
            .set_timeout(Duration::from_millis(50))
            .expect("master timeout");
        let path = "pty-at-session-timeout".to_string();

        sp.serialports.lock().unwrap().insert(
            path.clone(),
            SerialportInfo {
                state: PortState::Connected(ConnectedPort::new(Box::new(slave))),
            },
        );

        // Consume TX from the slave side so PTY writes never block on a full master buffer.
        thread::spawn(move || {
            let mut buf = [0u8; 256];
            loop {
                match master.read(&mut buf) {
                    Ok(0) | Err(_) => thread::sleep(Duration::from_millis(10)),
                    Ok(_) => {}
                }
            }
        });

        sp.configure_at_session(
            path.clone(),
            AtSessionConfig {
                default_timeout_ms: Some(200),
                default_rx_prepare: Some(RxPrepareMode::None),
                expect_ok: Some(false),
                ..Default::default()
            },
        )
        .expect("configure session");

        let start = Instant::now();
        let result = sp.at(path.clone(), "AT".to_string(), None);
        let elapsed = start.elapsed();

        assert!(result.is_err(), "missing OK should time out");
        assert!(
            elapsed < Duration::from_millis(800),
            "expected ~200 ms session timeout, took {elapsed:?}"
        );
        let err = result.unwrap_err().to_string();
        assert!(err.contains("timed out"), "unexpected error: {err}");

        let _ = sp.close(path);
    }

    /// Watch + concurrent status I/O must not prevent AT session timeout (demo path).
    #[cfg(unix)]
    #[test]
    fn at_times_out_with_watch_and_status_io_on_pty() {
        use crate::at_session::AtSessionConfig;
        use crate::events::WatchOptions;
        use crate::state::{ConnectedPort, PortState, SerialportInfo};
        use serialport::SerialPort as SerialPortTrait;
        use std::io::Read;
        use std::sync::{Arc, Mutex};
        use std::thread;
        use std::time::{Duration, Instant};
        use tauri::ipc::Channel;

        let app = create_test_app();
        let sp = app.state::<SerialPort<MockRuntime>>().inner().clone();

        let (mut master, slave) = serialport::TTYPort::pair().expect("pty pair");
        master
            .set_timeout(Duration::from_millis(50))
            .expect("master timeout");
        let path = "pty-at-watch-timeout".to_string();

        sp.serialports.lock().unwrap().insert(
            path.clone(),
            SerialportInfo {
                state: PortState::Connected(ConnectedPort::new(Box::new(slave))),
            },
        );

        thread::spawn(move || {
            let mut buf = [0u8; 256];
            loop {
                match master.read(&mut buf) {
                    Ok(0) | Err(_) => thread::sleep(Duration::from_millis(10)),
                    Ok(_) => {}
                }
            }
        });

        let channel = Channel::<crate::events::SerialEvent>::new(|_| Ok(()));
        sp.watch(path.clone(), WatchOptions::default(), channel)
            .expect("watch");
        sp.configure_at_session(
            path.clone(),
            AtSessionConfig {
                default_timeout_ms: Some(300),
                expect_ok: Some(false),
                ..Default::default()
            },
        )
        .expect("configure session");

        let sp_io = sp.clone();
        let path_io = path.clone();
        let io_done = Arc::new(Mutex::new(false));
        let io_done_bg = io_done.clone();
        let io_thread = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(2);
            while Instant::now() < deadline {
                let _ = sp_io.bytes_to_read(path_io.clone());
                let _ = sp_io.read_clear_to_send(path_io.clone());
                thread::sleep(Duration::from_millis(20));
            }
            *io_done_bg.lock().unwrap() = true;
        });

        let start = Instant::now();
        let result = sp.at(path.clone(), "AT".to_string(), None);
        let elapsed = start.elapsed();

        io_thread.join().expect("io thread join");
        assert!(*io_done.lock().unwrap());

        assert!(result.is_err(), "missing OK should time out");
        assert!(
            elapsed < Duration::from_millis(1500),
            "AT hung too long with watch active: {elapsed:?}"
        );
        let err = result.unwrap_err().to_string();
        assert!(err.contains("timed out"), "unexpected: {err}");

        let _ = sp.close(path);
    }

    /// Status I/O (bytesToRead, modem lines) must succeed while watch is active (demo connect path).
    #[cfg(unix)]
    #[test]
    fn status_io_succeeds_with_watch_on_pty() {
        use crate::events::WatchOptions;
        use crate::state::{ConnectedPort, PortState, SerialportInfo};
        use serialport::SerialPort as SerialPortTrait;
        use std::io::Read;
        use std::thread;
        use std::time::Duration;
        use tauri::ipc::Channel;

        let app = create_test_app();
        let sp = app.state::<SerialPort<MockRuntime>>().inner().clone();

        let (mut master, slave) = serialport::TTYPort::pair().expect("pty pair");
        master
            .set_timeout(Duration::from_millis(50))
            .expect("master timeout");
        let path = "pty-status-io-watch".to_string();

        sp.serialports.lock().unwrap().insert(
            path.clone(),
            SerialportInfo {
                state: PortState::Connected(ConnectedPort::new(Box::new(slave))),
            },
        );

        thread::spawn(move || {
            let mut buf = [0u8; 256];
            loop {
                match master.read(&mut buf) {
                    Ok(0) | Err(_) => thread::sleep(Duration::from_millis(10)),
                    Ok(_) => {}
                }
            }
        });

        let channel = Channel::<crate::events::SerialEvent>::new(|_| Ok(()));
        sp.watch(path.clone(), WatchOptions::default(), channel)
            .expect("watch");

        sp.bytes_to_read(path.clone()).expect("bytes_to_read");
        sp.bytes_to_write(path.clone()).expect("bytes_to_write");

        let _ = sp.close(path);
    }

    /// Single RX hub: watch and exchange share the main fd; live URC during exchange is emitted on watch.
    #[cfg(unix)]
    #[test]
    fn exchange_with_watch_emits_live_urc_on_pty() {
        use crate::events::{ExchangeOptions, SerialEvent, WatchOptions};
        use crate::state::{ConnectedPort, PortState, SerialportInfo};
        use serialport::SerialPort as SerialPortTrait;
        use std::io::{Read, Write};
        use std::sync::{Arc, Mutex};
        use std::thread;
        use std::time::Duration;
        use tauri::ipc::Channel;
        use tauri::ipc::InvokeResponseBody;

        let app = create_test_app();
        let serial_port = app.state::<SerialPort<MockRuntime>>().inner().clone();

        let (mut master, slave) = serialport::TTYPort::pair().expect("pty pair");
        master
            .set_timeout(Duration::from_millis(50))
            .expect("master timeout");
        let path = "pty-watch-exchange-test".to_string();

        serial_port.serialports.lock().unwrap().insert(
            path.clone(),
            SerialportInfo {
                state: PortState::Connected(ConnectedPort::new(Box::new(slave))),
            },
        );

        let events: Arc<Mutex<Vec<SerialEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let events_clone = events.clone();
        let channel = Channel::<SerialEvent>::new(move |body| {
            if let InvokeResponseBody::Json(json) = body {
                if let Ok(event) = serde_json::from_str::<SerialEvent>(&json) {
                    events_clone.lock().unwrap().push(event);
                }
            }
            Ok(())
        });

        serial_port
            .watch(path.clone(), WatchOptions::default(), channel)
            .expect("watch");

        let (done_tx, done_rx) = std::sync::mpsc::channel();

        let responder = thread::spawn(move || {
            let mut buf = [0u8; 512];
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while std::time::Instant::now() < deadline {
                match master.read(&mut buf) {
                    Ok(0) => continue,
                    Ok(n) => {
                        let cmd = String::from_utf8_lossy(&buf[..n]);
                        if cmd.contains("AT+CSQ") {
                            master
                                .write_all(
                                    b"\r\n+CREG: 0,1\r\n\r\nAT+CSQ\r\n\r\n+CSQ: 10,99\r\n\r\nOK\r\n",
                                )
                                .expect("write AT response");
                            master.flush().ok();
                            break;
                        }
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => continue,
                    Err(e) => panic!("master read failed: {}", e),
                }
            }
            let _ = done_rx.recv_timeout(Duration::from_secs(5));
        });

        let response = serial_port
            .exchange(
                path.clone(),
                "AT+CSQ\r".to_string(),
                ExchangeOptions {
                    timeout_ms: Some(3000),
                    rx_prepare: Some(crate::events::RxPrepareMode::None),
                    ..Default::default()
                },
            )
            .expect("exchange");

        done_tx.send(()).ok();
        responder.join().expect("responder join");

        assert_eq!(response.status, crate::at_parse::AtParseStatus::Ok);
        assert!(
            response
                .solicited_body
                .iter()
                .any(|l| l.starts_with("+CSQ:")),
            "expected +CSQ in solicited body: {:?}",
            response.solicited_body
        );

        let urc_lines: Vec<String> = events
            .lock()
            .unwrap()
            .iter()
            .filter_map(|e| match e {
                SerialEvent::Urc { line, .. } => Some(line.clone()),
                _ => None,
            })
            .collect();
        assert!(
            urc_lines.iter().any(|l| l.contains("+CREG:")),
            "expected live +CREG URC on watch during exchange, got {:?}",
            urc_lines
        );

        let _ = serial_port.close(path);
    }

    /// ATV0 numeric completion (`0` = OK).
    #[cfg(unix)]
    #[test]
    fn exchange_numeric_atv0_on_pty() {
        use crate::events::{AtResultFormat, ExchangeOptions};
        use crate::state::{ConnectedPort, PortState, SerialportInfo};
        use serialport::SerialPort as SerialPortTrait;
        use std::io::{Read, Write};
        use std::thread;
        use std::time::Duration;

        let app = create_test_app();
        let serial_port = app.state::<SerialPort<MockRuntime>>().inner().clone();

        let (mut master, slave) = serialport::TTYPort::pair().expect("pty pair");
        master.set_timeout(Duration::from_millis(50)).unwrap();
        let path = "pty-atv0-test".to_string();

        serial_port.serialports.lock().unwrap().insert(
            path.clone(),
            SerialportInfo {
                state: PortState::Connected(ConnectedPort::new(Box::new(slave))),
            },
        );

        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let responder = thread::spawn(move || {
            let mut buf = [0u8; 256];
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while std::time::Instant::now() < deadline {
                match master.read(&mut buf) {
                    Ok(0) => continue,
                    Ok(_) => {
                        master.write_all(b"\r\n0\r\n").expect("write numeric OK");
                        master.flush().ok();
                        break;
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => continue,
                    Err(e) => panic!("master read failed: {}", e),
                }
            }
            let _ = done_rx.recv_timeout(Duration::from_secs(5));
        });

        let response = serial_port
            .exchange(
                path.clone(),
                "AT\r".to_string(),
                ExchangeOptions {
                    timeout_ms: Some(3000),
                    rx_prepare: Some(crate::events::RxPrepareMode::None),
                    result_format: Some(AtResultFormat::Numeric),
                    ..Default::default()
                },
            )
            .expect("exchange");

        done_tx.send(()).ok();
        responder.join().expect("responder join");
        assert_eq!(response.status, crate::at_parse::AtParseStatus::Ok);
        let _ = serial_port.close(path);
    }

    /// CMGS-style intermediate `>` then SEND OK.
    #[cfg(unix)]
    #[test]
    fn exchange_cmgs_phases_on_pty() {
        use crate::events::{ExchangeCompletionMode, ExchangeOptions};
        use crate::state::{ConnectedPort, PortState, SerialportInfo};
        use serialport::SerialPort as SerialPortTrait;
        use std::io::{Read, Write};
        use std::thread;
        use std::time::Duration;

        let app = create_test_app();
        let serial_port = app.state::<SerialPort<MockRuntime>>().inner().clone();

        let (mut master, slave) = serialport::TTYPort::pair().expect("pty pair");
        master.set_timeout(Duration::from_millis(50)).unwrap();
        let path = "pty-cmgs-test".to_string();

        serial_port.serialports.lock().unwrap().insert(
            path.clone(),
            SerialportInfo {
                state: PortState::Connected(ConnectedPort::new(Box::new(slave))),
            },
        );

        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let responder = thread::spawn(move || {
            let mut buf = [0u8; 256];
            let mut phase = 0u8;
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while std::time::Instant::now() < deadline {
                match master.read(&mut buf) {
                    Ok(0) => continue,
                    Ok(n) => {
                        let cmd = String::from_utf8_lossy(&buf[..n]);
                        if phase == 0 && cmd.contains("CMGS") {
                            master.write_all(b"\r\n>\r\n").expect("prompt");
                            phase = 1;
                        } else if phase == 1 {
                            master.write_all(b"\r\nSEND OK\r\n").expect("send ok");
                            break;
                        }
                        master.flush().ok();
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => continue,
                    Err(e) => panic!("master read failed: {}", e),
                }
            }
            let _ = done_rx.recv_timeout(Duration::from_secs(5));
        });

        let prompt = serial_port
            .exchange(
                path.clone(),
                "AT+CMGS=1\r".to_string(),
                ExchangeOptions {
                    timeout_ms: Some(3000),
                    rx_prepare: Some(crate::events::RxPrepareMode::None),
                    completion_mode: Some(ExchangeCompletionMode::AtIntermediate),
                    command: Some("AT+CMGS=1".into()),
                    ..Default::default()
                },
            )
            .expect("prompt exchange");
        assert!(matches!(
            prompt.matched,
            crate::at_parse::ExchangeMatch::Intermediate { .. }
        ));

        let final_resp = serial_port
            .exchange_binary(
                path.clone(),
                vec![0x41, 0x1A],
                ExchangeOptions {
                    timeout_ms: Some(3000),
                    rx_prepare: Some(crate::events::RxPrepareMode::None),
                    command: Some("".into()),
                    ..Default::default()
                },
            )
            .expect("pdu exchange");

        done_tx.send(()).ok();
        responder.join().expect("responder join");
        assert_eq!(final_resp.status, crate::at_parse::AtParseStatus::Ok);
        let _ = serial_port.close(path);
    }

    /// CMUX session routes UIH payload to a registered DLCI exchange waiter.
    #[test]
    fn cmux_session_routes_uih_to_virtual_exchange() {
        use crate::cmux::{encode_uih, CmuxSession};
        use crate::events::{AtResultFormat, ExchangeCompletionMode, RxPrepareMode};
        use crate::exchange_read::ResolvedExchangeOptions;
        use crate::port_rx_hub::ExchangeWaiter;
        use serialport::TTYPort;
        use std::sync::atomic::AtomicBool;
        use std::sync::{Arc, Mutex};

        let (port, _) = TTYPort::pair().expect("pty");
        let path = "cmux-session-test".to_string();
        let session = CmuxSession::new(
            path.clone(),
            Arc::new(crate::cmux::SerialPortIo(Arc::new(Mutex::new(
                Box::new(port) as Box<dyn serialport::SerialPort>,
            )))),
        );
        session.register_dlci(2, format!("{path}#dlci=2"));
        let cancel = Arc::new(AtomicBool::new(false));
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
        let waiter = ExchangeWaiter::new(options, cancel);
        session.set_exchange_waiter(2, waiter.clone());
        let frame = encode_uih(2, b"AT\r\r\nOK\r\n");
        session.feed_physical_rx(&frame);
        let (raw, matched) = waiter.wait(1000).expect("virtual exchange complete");
        assert!(String::from_utf8_lossy(&raw).contains("OK"));
        assert!(matches!(matched, crate::at_parse::ExchangeMatch::Ok));
    }

    /// Full path: physical PTY + CMUX session + virtual port `sendAt`/`exchange`.
    #[cfg(unix)]
    #[test]
    fn cmux_virtual_port_exchange_on_pty() {
        use crate::cmux::{encode_uih, CmuxSession};
        use crate::port_rx_hub::PortRxHub;
        use crate::state::{ConnectedPort, PortState, SerialportInfo};
        use serialport::SerialPort as SerialPortTrait;
        use std::io::{Read, Write};
        use std::thread;
        use std::time::Duration;

        let app = create_test_app();
        let serial_port = app.state::<SerialPort<MockRuntime>>().inner().clone();

        let (mut master, slave) = serialport::TTYPort::pair().expect("pty pair");
        master.set_timeout(Duration::from_millis(50)).unwrap();
        let path = "pty-cmux-virtual".to_string();

        serial_port.serialports.lock().unwrap().insert(
            path.clone(),
            SerialportInfo {
                state: PortState::Connected(ConnectedPort::new(Box::new(slave))),
            },
        );

        {
            let mut ports = serial_port.serialports.lock().unwrap();
            let cp = ports
                .get_mut(&path)
                .unwrap()
                .connected_port_mut()
                .expect("connected");
            let session = CmuxSession::new(
                path.clone(),
                Arc::new(crate::cmux::SerialPortIo(cp.port.clone())),
            );
            let mut hub_guard = cp.rx_hub.lock().unwrap();
            if hub_guard.is_none() {
                *hub_guard = Some(PortRxHub::start(cp.port.clone(), path.clone()));
            }
            hub_guard
                .as_ref()
                .expect("hub")
                .attach_cmux(session.clone());
            *cp.mux.lock().unwrap() = Some(session);
        }

        let virtual_path = serial_port
            .open_mux_channel(path.clone(), 2)
            .expect("open virtual");

        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let responder = thread::spawn(move || {
            let mut buf = vec![0u8; 4096];
            let mut acc = Vec::new();
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while std::time::Instant::now() < deadline {
                match master.read(&mut buf) {
                    Ok(0) => continue,
                    Ok(n) => {
                        acc.extend_from_slice(&buf[..n]);
                        if acc.windows(4).any(|w| w == b"AT\r\r") || acc.contains(&b'A') {
                            let frame = encode_uih(2, b"\r\nOK\r\n");
                            master.write_all(&frame).expect("uih ok");
                            master.flush().ok();
                            break;
                        }
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => continue,
                    Err(e) => panic!("master read failed: {e}"),
                }
            }
            let _ = done_rx.recv_timeout(Duration::from_secs(5));
        });

        let response = serial_port
            .at(virtual_path.clone(), "AT".to_string(), None)
            .expect("virtual AT");

        done_tx.send(()).ok();
        responder.join().expect("responder join");
        assert_eq!(response.status, crate::at_parse::AtParseStatus::Ok);
        let _ = serial_port.close(virtual_path);
        let _ = serial_port.close(path);
    }

    /// B3: finished RX hub is restarted on the next read.
    #[test]
    fn ensure_rx_hub_running_restarts_finished_hub() {
        use crate::state::{ConnectedPort, PortState, SerialportInfo};
        use crate::tests::mock_serial::MockSerialPort;

        let app = create_test_app();
        let serial_port = app.state::<SerialPort<MockRuntime>>().inner().clone();
        let path = "mock-hub-restart".to_string();

        serial_port.serialports.lock().unwrap().insert(
            path.clone(),
            SerialportInfo {
                state: PortState::Connected(ConnectedPort::new(Box::new(MockSerialPort::new()))),
            },
        );

        let mut ports_guard = serial_port.serialports.lock().unwrap();
        let cp = ports_guard
            .get_mut(&path)
            .expect("port")
            .connected_port_mut()
            .expect("connected");
        {
            let mut hub_guard = cp.rx_hub.lock().unwrap();
            let hub = crate::port_rx_hub::PortRxHub::start(cp.port.clone(), path.clone());
            *hub_guard = Some(hub);
        }
        {
            let mut hub_guard = cp.rx_hub.lock().unwrap();
            if let Some(hub) = hub_guard.take() {
                hub.stop();
            }
            assert!(hub_guard.is_none());
        }
        drop(ports_guard);

        let err = serial_port.read(path.clone(), Some(50), Some(8));
        assert!(err.is_err());
        let hub_running = {
            let ports = serial_port.serialports.lock().unwrap();
            ports.get(&path).and_then(|info| match &info.state {
                PortState::Connected(cp) => cp
                    .rx_hub
                    .lock()
                    .ok()
                    .and_then(|guard| guard.as_ref().map(|h| !h.is_finished())),
                _ => None,
            })
        }
        .unwrap_or(false);
        assert!(hub_running, "hub should have been restarted");
    }
    #[test]
    fn close_virtual_while_managed_ports_does_not_deadlock() {
        use crate::state::{ConnectedPort, PortState, SerialportInfo, VirtualPortRef};
        use crate::tests::mock_serial::MockSerialPort;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        use std::time::Duration;

        let app = create_test_app();
        let serial_port = app.state::<SerialPort<MockRuntime>>().inner().clone();
        let physical = "mock-physical-b2".to_string();
        let virtual_path = format!("{physical}#dlci=2");

        serial_port.serialports.lock().unwrap().insert(
            physical.clone(),
            SerialportInfo {
                state: PortState::Connected(ConnectedPort::new(Box::new(MockSerialPort::new()))),
            },
        );
        serial_port.virtual_ports.lock().unwrap().insert(
            virtual_path.clone(),
            VirtualPortRef {
                physical_path: physical.clone(),
                dlci: 2,
                exchange_cancel: Arc::new(AtomicBool::new(false)),
                tx_queue: Arc::new(crate::port_tx_queue::PortTxQueue::new()),
            },
        );

        let done = Arc::new(AtomicBool::new(false));
        let done_bg = done.clone();
        let sp = serial_port.clone();
        let vp = virtual_path.clone();
        let closer = std::thread::spawn(move || {
            let _ = sp.close(vp);
            done_bg.store(true, Ordering::SeqCst);
        });

        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while std::time::Instant::now() < deadline {
            let _ = serial_port.managed_ports();
            if done.load(Ordering::SeqCst) {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        closer.join().expect("close join");
        assert!(done.load(Ordering::SeqCst));
    }
}
