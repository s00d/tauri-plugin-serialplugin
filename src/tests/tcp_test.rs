//! Serial-over-TCP (`tcp://host:port`) end to end against a local listener:
//! open, write, exchange, watch, and disconnect when the bridge goes away.

#[cfg(test)]
mod tests {
    use crate::api::SerialPort;
    use crate::events::{ExchangeOptions, SerialEvent, WatchOptions};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::time::Duration;
    use tauri::ipc::{Channel, InvokeResponseBody};
    use tauri::Manager;

    #[test]
    fn tcp_bridge_open_exchange_watch_and_disconnect() {
        let app = tauri::test::mock_app();
        app.manage(SerialPort::new(app.handle().clone()));
        let serial = app
            .state::<SerialPort<tauri::test::MockRuntime>>()
            .inner()
            .clone();

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let path = format!("tcp://{}", listener.local_addr().unwrap());
        // Fake modem behind a raw TCP bridge: answers AT, then pushes a line and hangs up.
        let bridge = std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            let mut buf = [0u8; 64];
            let n = sock.read(&mut buf).unwrap();
            assert_eq!(&buf[..n], b"AT\r");
            sock.write_all(b"AT\r\r\nOK\r\n").unwrap();
            let n = sock.read(&mut buf).unwrap();
            assert_eq!(&buf[..n], b"go");
            sock.write_all("Привет\r\n".as_bytes()).unwrap();
            std::thread::sleep(Duration::from_millis(200));
        });

        assert_eq!(
            serial
                .open(path.clone(), 115_200, None, None, None, None, Some(200))
                .unwrap(),
            path
        );
        let resp = serial
            .exchange(
                path.clone(),
                "AT\r".into(),
                ExchangeOptions {
                    timeout_ms: Some(2000),
                    ..Default::default()
                },
            )
            .expect("exchange over tcp");
        assert_eq!(resp.status, crate::at::parse::AtParseStatus::Ok);

        let (tx, rx) = mpsc::channel::<SerialEvent>();
        let tx = std::sync::Mutex::new(tx);
        let channel = Channel::<SerialEvent>::new(move |body| {
            if let InvokeResponseBody::Json(json) = body {
                let _ = tx
                    .lock()
                    .unwrap()
                    .send(serde_json::from_str(&json).unwrap());
            }
            Ok(())
        });
        serial
            .watch(path.clone(), WatchOptions::default(), channel)
            .unwrap();
        serial.write(path.clone(), "go".into()).unwrap();

        let mut data = Vec::new();
        let mut disconnected = false;
        while let Ok(event) = rx.recv_timeout(Duration::from_secs(3)) {
            match event {
                SerialEvent::Data { data: d, .. } => data.extend(d),
                SerialEvent::Disconnect { .. } => {
                    disconnected = true;
                    break;
                }
                _ => {}
            }
        }
        bridge.join().unwrap();
        assert_eq!(String::from_utf8(data).unwrap(), "Привет\r\n");
        assert!(
            disconnected,
            "closing the TCP bridge must end in Disconnect"
        );
        serial.close(path).unwrap();
    }

    #[test]
    fn tcp_open_refused_is_an_error() {
        let app = tauri::test::mock_app();
        app.manage(SerialPort::new(app.handle().clone()));
        let serial = app
            .state::<SerialPort<tauri::test::MockRuntime>>()
            .inner()
            .clone();
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let err = serial
            .open(
                format!("tcp://127.0.0.1:{port}"),
                9600,
                None,
                None,
                None,
                None,
                None,
            )
            .expect_err("nothing listens there");
        assert!(err.to_string().contains("tcp://"), "{err}");
        assert!(serial.managed_ports().unwrap().is_empty());
    }
}
