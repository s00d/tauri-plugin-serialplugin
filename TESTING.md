# Testing

Behaviour is tested the way users use it: open a port, watch it, talk AT, start a multiplexer,
lose the device, reopen, close; for the plugin, call its IPC commands as a webview does. Tests
go through the public API only.

## Kinds

| Kind | What | Example |
|---|---|---|
| **model** (proptest) | Random action sequences (or inputs) checked against a small model of the rules: the model predicts every result and every event. Invalid actions are part of it. On failure proptest shrinks to the shortest sequence. | `crates/serialplugin-core/tests/it/model_manager.rs` |
| **story** | One user journey per test, start to end, with a `Given / When / Then` doc comment. It checks every step's observable result, not one function. | `crates/serialplugin-core/tests/it/story_*.rs` |
| **corpus** | Vectors recorded from a reference: 3GPP examples, other implementations, the Java USB drivers' control transfers. | `crates/android-usb-serial/tests/it/corpus_golden.rs` |

## Fakes

- `serialplugin-testing`: an in-memory port (`FakeDevice`), an AT modem with a
  27.010 multiplexer (`Modem`), PTY and TCP peers (`peer`). Written from the protocol
  descriptions, not with the codecs under test. Used by serialplugin-core and the plugin.
- `android-usb-serial` drives its drivers through its own `FakeTransport` (feature
  `fake-transport`).
- A real modem: `SERIALPLUGIN_MODEM=/dev/cu.… cargo nextest run -p tauri-plugin-serialplugin story_modem`
  (queries only; without the variable it prints SKIP and passes).

## Layout

Each crate has **one** integration test binary, `tests/it/main.rs`, whose modules are the
`model_*`, `story_*` and `corpus_*` files, plus `tests/it/common/` for helpers. No
`#[cfg(test)] mod tests` in `src/` (doctests stay: they are documentation), except checks the
public API cannot make; each such module says why in its first line.

## Rules

- Compare payloads, states, error kinds, events and the bytes the device saw. Compare message
  texts only where the text is the contract (4.x error strings).
- A new regression is a new step in a story or a case the model now generates, never a new
  one-function test.
- A step that cannot be observed in the current state is skipped by the model, not by a
  generator filter. Every case starts clean: what a case leaves behind is removed in `Drop`.
- No timing: no sleeps, no real-time limits. Wait for events (an event on a channel, a command
  the modem logged, a held device call); `WAIT` only bounds how long a failure takes. Delays a
  test needs (flush interval, T1) are small settings, not sleeps in the test. "Nothing arrives"
  is proved by a later event that did. Where no event announces a state, `eventually` polls
  for it; its interval (1 ms) is no synchronisation.
- Bytes the host read are not yet bytes it handled: before a step that depends on it, wait for
  `FakeDevice::wait_handled`, or for a second stream that got them.
- Per-test timeouts are in `.config/nextest.toml`. CI runs `cargo test` without per-test
  limits: there a hang costs the job's timeout, so run nextest before pushing.
- Logs of long runs go to `target/logs/<task>.log`.

## Running

```bash
cargo nextest run --workspace --features android-usb-serial/fake-transport,android-usb-serial/serialport-compat
cargo nextest run -p serialplugin-core                          # one crate
cargo nextest run -p serialplugin-core -E 'test(/^story_mux::/)'   # one area
```
