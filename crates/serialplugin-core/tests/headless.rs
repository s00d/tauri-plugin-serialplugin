//! The core with no Tauri at all: open a `tcp://` bridge to a tiny fake modem,
//! watch it through a closure sink with a native `readline` parser, run `at()`, close.

use serialplugin_core::at::parse::AtParseStatus;
use serialplugin_core::events::{EventSink, SerialEvent, WatchOptions};
use serialplugin_core::SerialManager;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

/// Answers one `\r`-terminated command per read: `go` pushes two lines, `AT+CSQ` replies.
fn fake_modem(listener: TcpListener) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let (mut sock, _) = listener.accept().unwrap();
        let mut pending = Vec::new();
        let mut buf = [0u8; 256];
        loop {
            let n = match sock.read(&mut buf) {
                Ok(0) | Err(_) => return,
                Ok(n) => n,
            };
            pending.extend_from_slice(&buf[..n]);
            while let Some(end) = pending.iter().position(|&b| b == b'\r') {
                let cmd: Vec<u8> = pending.drain(..=end).collect();
                let reply: &[u8] = match &cmd[..cmd.len() - 1] {
                    b"go" => b"hello\r\nworld\r\n",
                    b"AT+CSQ" => b"\r\n+CSQ: 20,99\r\n\r\nOK\r\n",
                    _ => b"\r\nERROR\r\n",
                };
                sock.write_all(reply).unwrap();
            }
        }
    })
}

#[test]
fn open_watch_with_parser_at_and_close_without_tauri() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let path = format!("tcp://{}", listener.local_addr().unwrap());
    let modem = fake_modem(listener);

    let serial = SerialManager::new();
    assert_eq!(
        serial
            .open(path.clone(), 115_200, None, None, None, None, Some(200))
            .unwrap(),
        path
    );

    let (tx, rx) = mpsc::channel::<SerialEvent>();
    let tx = Mutex::new(tx);
    let sink: EventSink = Arc::new(move |event| tx.lock().unwrap().send(event).is_ok());
    let options = WatchOptions {
        parser: Some(
            serde_json::from_value(serde_json::json!({"type": "readline", "delimiter": "\r\n"}))
                .unwrap(),
        ),
        ..Default::default()
    };
    let watch_id = serial.watch(path.clone(), options, sink).unwrap();

    serial.write(path.clone(), "go\r".into()).unwrap();
    let mut lines = Vec::new();
    while lines.len() < 2 {
        match rx
            .recv_timeout(Duration::from_secs(3))
            .expect("watch frame")
        {
            SerialEvent::Data { data, size, .. } => {
                assert_eq!(size, data.len());
                lines.push(String::from_utf8(data).unwrap());
            }
            other => panic!("unexpected event {other:?}"),
        }
    }
    assert_eq!(lines, ["hello", "world"]);

    let reply = serial.at(path.clone(), "AT+CSQ".into(), None).unwrap();
    assert_eq!(reply.status, AtParseStatus::Ok, "{reply:?}");
    assert!(reply.lines.iter().any(|l| l == "+CSQ: 20,99"), "{reply:?}");

    serial.unwatch(watch_id).unwrap();
    serial.close(path).unwrap();
    assert!(serial.managed_ports().unwrap().is_empty());
    modem.join().unwrap();
}
