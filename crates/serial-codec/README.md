# serial-codec

Byte-stream codecs for serial ports, with no runtime or framework dependencies.
Used by [`tauri-plugin-serialplugin`](https://github.com/s00d/tauri-plugin-serialplugin),
usable on its own.

- `framing`: turn read chunks into frames. Same semantics as the Node.js
  `@serialport/parser-*` packages (their test suites are ported): `delimiter`,
  `readline`, `byteLength`, `interByteTimeout`, `packetLength`, `ready`, `regex`,
  `startEnd`, `slip` (plus `slip_encode`) and `ccTalk`.
- `at`: AT command response parsing (final result codes, URCs, verbose/numeric).
  Result codes are those of V.250, 27.007 (`+CME ERROR`) and 27.005 (`+CMS ERROR`),
  plus `SEND OK` / `SEND FAIL` (not V.250: the finals of module TCP stacks, kept for
  compatibility). Numeric mode (`ATV0`) accepts CR-only result codes (V.250 §5.7.1).
- `urc` / `args`: typed 3GPP TS 27.007 / 27.005 line parsers (registration, signal, calls,
  SMS headers, operators, `+CGEV`) and a quote-aware argument splitter.
- `cmux`: 3GPP TS 27.010 (GSM 07.10) CMUX frame encode/decode.
- `sms`: 3GPP TS 23.038 alphabets (GSM 7-bit with national tables, UCS2) and TS 23.040
  PDUs for `AT+CMGF=0`: decode SMS-DELIVER / SUBMIT / STATUS-REPORT, encode SMS-SUBMIT with
  automatic concatenation, multipart reassembly.
- `ussd`: USSD 7-bit packing, CBS data coding schemes and the `+CUSD` line.

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
bytes (`encoding` is accepted and ignored), buffered bytes are bounded
(`maxBufferSize`, default 64 KiB, for every framer that buffers, `packetLength`
included), the work per pushed byte is bounded (a stream without delimiters fed one
byte at a time stays linear), unknown option names are rejected, empty frames are
never emitted, `ready` and multi-byte `packetLength` delimiters are found after a
false start, and `regex` keeps UTF-8 characters split across reads.

## URCs and responses

`urc` parses one already-framed line into a serde-serializable (camelCase) type; each parser
returns `None` for another command or a malformed line. `urc::parse_any` dispatches the
unsolicited ones (`+CREG`/`+CGREG`/`+CEREG`/`+C5GREG`, `+CLIP`, `+COLP`, `+CCWA`, `+CIEV`,
`+CMTI`, `+CDSI`, `+CMT`, `+CDS`, `+CGEV`). Response parsers: `Csq`, `Cesq`, `Cpin`, `Clcc`,
`CindValues`/`CindRanges`, `MessageHeader` (`+CMGL`/`+CMGR`, header only: the body is the next
line), `MessageReference` (`+CMGS`/`+CMSS`), `CpmsStatus`/`CpmsSupport`, `Cnmi`/`CnmiSupport`,
`Cops`/`CopsList`. `+CUSD` lives in the USSD module.

```rust
use serial_codec::urc::{parse_any, Csq, RegStatus, Urc};

// `<n>,<stat>,...` (read response) and `<stat>,...` (URC) are told apart.
let Some(Urc::Registration(r)) = parse_any("+CREG: 2,5,\"0502\",\"0404736D\",2") else { panic!() };
assert_eq!((r.n, r.stat, r.lac), (Some(2), RegStatus::Roaming, Some(0x0502)));

assert_eq!(Csq::parse("+CSQ: 18,99").unwrap().rssi_dbm(), Some(-77));
```

`args::split` is the underlying splitter: commas inside quotes or parentheses do not split,
`\HH` escapes are decoded (V.250 §5.4.2.2: a quote is `\22`; any other backslash is text),
omitted fields stay as empty arguments.

## SMS and USSD

```rust
use serial_codec::sms::pdu::{decode_hex, encode_submit, SubmitRequest, Tpdu};
use serial_codec::sms::concat::Reassembler;

// Decode what +CMGR / +CMT printed (SMSC prefix included).
let pdu = decode_hex("07911326040000F0040B910100000020F70000208062917314480CC8F71D14969741F977FD07").unwrap();
let Tpdu::Deliver(d) = &pdu.tpdu else { panic!() };
assert_eq!(d.originator.to_string(), "+10000000027");
assert_eq!(pdu.tpdu.text(), Some("How are you?"));
assert_eq!(d.scts.unwrap().to_string(), "2002-08-26T19:37:41-01:00");

// Encode: one (length for AT+CMGS, hex) pair per part. A message that needs more than
// one part needs a concatenation reference (keep a counter per recipient so two long
// messages in flight never share one); without it encode_submit returns
// PduError::MissingConcatRef.
let mut req = SubmitRequest::new("+15551234567", "x".repeat(200));
req.concat_ref = Some(42);
let parts = encode_submit(&req).unwrap();
assert_eq!(parts.len(), 2);

// Reassemble (also accepts parts out of order). At most 64 incomplete messages are held
// (Reassembler::with_max_pending to change it), the oldest is evicted first; call
// expire_before periodically.
let mut asm = Reassembler::new();
let text = parts
    .iter()
    .find_map(|(_, hex)| asm.push_tpdu(&decode_hex(hex).unwrap().tpdu, 0))
    .and_then(|m| m.text);
assert_eq!(text, Some("x".repeat(200)));

// +CUSD with a GSM 7-bit DCS: <str> is in the +CSCS character set (27.007 §7.15).
use serial_codec::ussd::{parse_cusd, Cscs};
let c = parse_cusd(r#"+CUSD: 0,"Balance 5.00",15"#).unwrap();
assert_eq!(c.text().as_deref(), Some("Balance 5.00")); // +CSCS="GSM"/"IRA"
let c = parse_cusd(r#"+CUSD: 0,"00310032",15"#).unwrap();
assert_eq!(c.text().as_deref(), Some("00310032"));
assert_eq!(c.text_as(Cscs::Ucs2).as_deref(), Some("12")); // +CSCS="UCS2"
// Some firmware (Huawei E160, ...) sends packed septets in hex instead: non-standard, opt in.
let c = parse_cusd(r#"+CUSD: 0,"AA58ACA6AA8D1A",15"#).unwrap();
assert_eq!(c.text_packed_hex().as_deref(), Some("*115*5#"));
```

Decoding is strict about lengths (TP-UDL beyond the data, odd UCS2 payloads and truncated
fields are errors) and tolerant where 23.038/23.040 ask receivers to be: reserved DCS values
read as GSM 7-bit, any TOA octet is accepted, an invalid SCTS decodes as `None` (raw octets
kept), an unparseable optional TP-PI tail of a status report is ignored (the mandatory
fields stay). A `+CUSD` DCS that is not 0–255 is kept in `invalid_dcs`; `CbsDcs::language`
gives the language a CBS DCS names. The encoder picks GSM 7-bit, then the national tables you allow
(`locking_shift`/`single_shift`) when they avoid UCS2, then UCS2; parts never split an escape
pair or a surrogate pair, references above 255 switch to the 16-bit IE. National tables:
Turkish, Spanish (single shift), Portuguese; the Indic tables of 23.038 Annex A are not
included. Fixture sources and the known disagreements with other implementations are listed
in `tests/fixtures/sms/README.md` and `tests/fixtures/ussd/README.md`.

Minimum Rust version: 1.85.

## License

[MPL-2.0](LICENSE). You can use the crate in any project, open or closed source,
under any license. If you distribute a modified version of this crate's files,
those files must stay MPL-2.0 and their source must be published.
