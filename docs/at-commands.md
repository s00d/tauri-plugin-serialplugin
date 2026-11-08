# AT commands, exchange and CMUX

For modems and other request/response devices the plugin has a native
transaction queue: every `exchange()`, `sendAt()`, `sendAtPhases()` and
`sendSmsPdu()` call waits its turn, writes, and resolves when the reply is
complete. Concurrent calls never interleave on the wire, and a `watch()` can stay
active the whole time to receive unsolicited lines.

- [`sendAt()`](#sendat)
- [Session defaults](#session-defaults)
- [Multi-phase commands and SMS](#multi-phase-commands-and-sms)
- [`exchange()` for non-AT protocols](#exchange-for-non-at-protocols)
- [Cancelling](#cancelling)
- [URCs while commands run](#urcs-while-commands-run)
- [CMUX (GSM 07.10 / 3GPP 27.010)](#cmux-gsm-0710--3gpp-27010)

## `sendAt()`

```ts
const r = await port.sendAt("AT+CSQ");
// r.status: "ok" | "error" | "cme" | "cms" | "unknown"
// r.lines:  ["AT+CSQ", "+CSQ: 27,0", "OK"]
// r.solicitedBody: ["+CSQ: 27,0"]   r.urcLines: []   r.raw: Uint8Array
```

A command is complete when its final result line arrives: `OK`, `ERROR`,
`+CME ERROR: n`, `+CMS ERROR: n`, `NO CARRIER`, `BUSY`, `NO ANSWER`,
`NO DIALTONE`, `SEND OK`, `SEND FAIL`. URCs that arrive in the same read (before
the echo, inside the reply, or right after `OK`) are reported separately and do
not end the command early.

`AtCommandResult`: `command`, `response`, `status`, `lines`, `solicitedBody`,
`urcLines`, `matched`, `timedOut`, `raw`.

`AtCommandOptions` (per call): `timeoutMs`, `resultFormat` (`"verbose"` or
`"numeric"` for `ATV0`), `completionMode`, `solicitedPrefixes`, `rxPrepare`,
`appendCr`, `terminators`, `idleMs`.

`\r` is appended unless the command already ends with CR/LF or `appendCr` is
`false`. Solicited prefixes are derived from the command (`AT+CSQ` → `+CSQ:`,
also `^`, `#`, `$`, `%`, `*` vendor prefixes); pass `solicitedPrefixes` when the
reply uses a different one.

## Session defaults

```ts
await port.configureAtSession({
  defaultTimeoutMs: 5000,
  expectOk: true,      // reject when status is not "ok"
  stopOnError: true,   // sendAtPhases: stop at the first failing phase
  appendCr: true,
  resultFormat: "verbose",
});
```

Pass the same object as `atSession` to the constructor to apply it on every
`open()`.

Before each command the queue soft-drains stale input (`rxPrepare: "drain"`, the
default): bytes left from an earlier timed-out reply are discarded, URCs in them
are still emitted. Use `"purge"` to drop the OS buffers too, or `"none"` to keep
everything.

## Multi-phase commands and SMS

`sendAtPhases()` runs several writes as one queue job, so nothing else can slip
in between (for example between `AT+CMGS` and the PDU):

```ts
const results = await port.sendAtPhases([
  { write: "AT+CMGS=23", completionMode: "atIntermediate" }, // waits for "> "
  { write: pduBytes /* Uint8Array incl. Ctrl+Z */ },
]);
```

`sendSmsPdu(length, pdu, { timeoutMs? })` is the built-in version of exactly that:
it waits for the `> ` prompt, sends the PDU with Ctrl+Z and resolves on
`+CMGS: n` / `OK`. If the modem answers `AT+CMGS` with `+CMS ERROR`, it fails
right away instead of waiting for the timeout.

## `exchange()` for non-AT protocols

`exchange(text, options)` / `exchangeBinary(bytes, options)` write and read until a
condition, through the same queue:

| `completionMode` | Completes when |
|------------------|----------------|
| `"atFinalLine"` (default) | A final AT result line arrives |
| `"atIntermediate"` | A prompt (`>`, `CONNECT`) or a final line arrives |
| `"substring"` | One of `terminators` appears anywhere |

Plus `idleMs` (done after this much silence), `maxBytes` and `timeoutMs`.
The result is an `ExchangeResponse` (`raw`, `matched`, `status`, `lines`,
`solicitedBody`, `urcLines`).

## Cancelling

`cancelExchange()` (alias `cancelAt()`) fails the running job with
`exchange cancelled`, rejects every queued job with
`transaction queue cancelled`, and leaves the queue ready for the next call. A
cancel that lands between the phases of `sendAtPhases()` / `sendSmsPdu()` stops
the remaining phases.

## URCs while commands run

Keep a watch with `onUrc` to get unsolicited lines live, including the ones
that arrive while a command is in flight:

```ts
await port.watch({
  onData: () => {},
  onUrc: (line) => console.log("URC", line), // +CMTI: "SM",3 / RING / +CREG: 1
});
await port.sendAt("AT+CSQ"); // the watch keeps running
```

## CMUX (GSM 07.10 / 3GPP 27.010)

CMUX splits one modem port into several logical channels (for example AT
commands on one, PPP data on another). The plugin speaks the basic option of
27.010: `AT+CMUX=0`, SABM/UA per channel, CLD on teardown.

```ts
await port.enableMux();                       // sends AT+CMUX=0,0,5,31,10,2 and opens DLCI 0
const at = await port.openMuxChannel(1);      // a SerialPort for DLCI 1 (1..=63)
const data = await port.openMuxChannel(2);
await at.sendAt("ATI");
await port.disableMux();                      // CLD: the modem returns to AT mode
```

- `enableMux({ command?, timeoutMs? })` fails if the modem answers `ERROR` or does
  not acknowledge the control channel.
- Virtual channels are paths like `/dev/ttyUSB0#dlci=2` and support `sendAt`,
  `exchange`, `write` and `watch`.
- Closing the physical port closes its channels and sends CLD.
- Frames carry at most 31 bytes (the 27.010 default N1); larger writes are split.

Tested against a Quectel M26.
