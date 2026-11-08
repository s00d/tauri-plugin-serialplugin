# serial-codec

Byte-stream codecs for serial ports, with no runtime or framework dependencies.
Used by [`tauri-plugin-serialplugin`](https://github.com/s00d/tauri-plugin-serialplugin),
usable on its own.

- `framing`: turn read chunks into frames. Same semantics as the Node.js
  `@serialport/parser-*` packages (their test suites are ported): `delimiter`,
  `readline`, `byteLength`, `interByteTimeout`, `packetLength`, `ready`, `regex`,
  `startEnd`, `slip` (plus `slip_encode`) and `ccTalk`.
- `at`: AT command response parsing (final result codes, URCs, verbose/numeric).
- `cmux`: 3GPP TS 27.010 (GSM 07.10) CMUX frame encode/decode.

## Framing

```rust
use serial_codec::framing::{Framer, FramerConfig};
use std::time::Instant;

let cfg: FramerConfig = serde_json::from_str(r#"{ "type": "readline", "delimiter": "\r\n" }"#).unwrap();
let mut framer = cfg.build().unwrap();

let mut frames = Vec::new();
framer.push(b"OK\r\nRI", Instant::now(), &mut frames);
framer.push(b"NG\r\n", Instant::now(), &mut frames);
assert_eq!(frames, vec![b"OK".to_vec(), b"RING".to_vec()]);
```

Time-based framers (`interByteTimeout`, `ccTalk`) take `now` instead of running
timers: call `poll(now, &mut frames)` when `deadline()` passes. `flush()` returns
the partial frame at end of stream.

Differences from Node (all deliberate, see the `framing` module docs): frames are
bytes (no `encoding`), buffers are bounded (`maxBufferSize`, default 64 KiB), empty
frames are never emitted, `ready` and multi-byte `packetLength` delimiters are found
after a false start, and `regex` keeps UTF-8 characters split across reads.

Minimum Rust version: 1.85.

## License

[MPL-2.0](LICENSE). You can use the crate in any project, open or closed source,
under any license. If you distribute a modified version of this crate's files,
those files must stay MPL-2.0 and their source must be published.
