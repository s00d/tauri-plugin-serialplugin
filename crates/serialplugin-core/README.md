# serialplugin-core

The serial port core of [`tauri-plugin-serialplugin`](https://github.com/s00d/tauri-plugin-serialplugin),
with no Tauri dependency. Use it from a plain Rust / tokio app (gateways, daemons,
CLIs) that talks to many ports at once; the Tauri plugin is a thin layer over it.

- One `SerialManager` owns every open port: local serial (`serialport`),
  `tcp://host:port` bridges, `ble://…` (feature `ble`, desktop + iOS) and, on
  Android, USB adapters through the plugin's JNI bridge.
- One RX hub thread per port: `watch` streams, poll `read`, request/response
  `exchange` and queued `at` / `at_phases` / `send_sms_pdu` never steal each
  other's bytes. URCs can be split out of the stream (`WatchOptions::route_urc`).
- Native framing on watch (`WatchOptions::parser`, the
  [`serial-codec`](../serial-codec) parsers: readline, delimiter, SLIP, …): each
  frame is one `SerialEvent::Data`.
- GSM 07.10 CMUX: `enable_mux`, `open_mux_channel` give virtual ports
  (`<path>#dlci=N`) that take the same calls as physical ones.

## Example

```rust,no_run
use serialplugin_core::events::{EventSink, SerialEvent, WatchOptions};
use serialplugin_core::SerialManager;
use std::sync::Arc;

let serial = SerialManager::new();
let path = serial.open("/dev/ttyUSB0".into(), 115_200, None, None, None, None, None)?;

// Called on the port's RX thread: keep it short (forward into a channel).
let sink: EventSink = Arc::new(|event| {
    if let SerialEvent::Data { path, data, .. } = event {
        println!("{path}: {}", String::from_utf8_lossy(&data));
    }
    true // false = receiver gone
});
let options = WatchOptions {
    parser: Some(serde_json::from_str(r#"{ "type": "readline", "delimiter": "\r\n" }"#).unwrap()),
    ..Default::default()
};
let watch_id = serial.watch(path.clone(), options, sink)?;

let csq = serial.at(path.clone(), "AT+CSQ".into(), None)?;
println!("{:?} {:?}", csq.status, csq.lines);

serial.unwatch(watch_id)?;
serial.close(path)?;
# Ok::<(), serialplugin_core::Error>(())
```

`cargo run -p serialplugin-core --example headless -- /dev/ttyUSB0 115200` prints every
line a port sends.

Methods block (open, read with a timeout, `at`): in async code call them through
`tokio::task::spawn_blocking`. `SerialManager` is cheap to clone; clones share the
ports. Port-list changes: `watch_ports(options, sink)` with a `PortListSink`.

## License

Apache-2.0 OR MIT.
