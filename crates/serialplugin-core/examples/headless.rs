//! Print every line a port sends, without Tauri.
//!
//! `cargo run -p serialplugin-core --example headless -- /dev/ttyUSB0 [baud]`
//! (any path `open` takes works: `COM3`, `tcp://host:port`, `ble://…` with the `ble` feature).

use serialplugin_core::events::{EventSink, SerialEvent, WatchOptions};
use serialplugin_core::SerialManager;
use std::sync::Arc;

fn main() -> Result<(), serialplugin_core::Error> {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: headless <path> [baud]");
    let baud = args.next().map_or(115_200, |b| b.parse().expect("baud"));

    let serial = SerialManager::new();
    let path = serial.open(path, baud, None, None, None, None, None)?;
    let sink: EventSink = Arc::new(|event| {
        match event {
            SerialEvent::Data { data, .. } => println!("{}", String::from_utf8_lossy(&data)),
            SerialEvent::Disconnect { path, reason } => {
                eprintln!("{path} disconnected: {reason}");
                std::process::exit(1);
            }
            other => eprintln!("{other:?}"),
        }
        true
    });
    let options = WatchOptions {
        parser: Some(
            serde_json::from_value(serde_json::json!({"type": "readline", "delimiter": "\r\n"}))
                .expect("parser config"),
        ),
        ..Default::default()
    };
    serial.watch(path, options, sink)?;
    loop {
        std::thread::park();
    }
}
