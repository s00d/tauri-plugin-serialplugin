# Test fixtures

Vectors taken from other projects' test suites: inputs and the outputs those tests
assert. Only the data is copied. No code, comments or test structure comes from these
projects. Every entry keeps `source` (`project/path:line`) and `license`.

| file | used by | sources |
|---|---|---|
| `cmux.json` | `tests/cmux_vectors.rs` | zephyr `tests/subsys/modem/modem_cmux` (Apache-2.0), oFono `unit/test-mux.c` (GPL-2.0-only) |
| `at_replies.json` | `tests/at_vectors.rs` | ModemManager `src/tests/test-modem-helpers.c`, `test-at-serial-port.c` (GPL-2.0-or-later), gammu `tests/` (GPL-2.0-only), chan_dongle `test/parse.c` (GPL-2.0-only), python-gsmmodem `test/` (LGPL-3.0-or-later) |
| `at_urc.json` | `tests/at_vectors.rs` | ModemManager (GPL-2.0-or-later), chan_dongle (GPL-2.0-only), python-gsmmodem (LGPL-3.0-or-later) |
| `cme_cms.json` | `tests/at_vectors.rs` | ModemManager `include/ModemManager-errors.h` (LGPL-2.0-or-later), `src/tests/test-error-helpers.c`, `src/mm-error-helpers.c` (GPL-2.0-or-later), gammu (GPL-2.0-only), python-gsmmodem (LGPL-3.0-or-later) |

## Fields

- `input`, `expected`, `note`, `name`: as the source has them. `expected` holds what the
  source asserts, which is often field-level parsing (for example `+COPS=?` networks or
  `+CREG` state). The crate doesn't parse those fields yet. Those values are kept for
  future typed parsers.
- `group` (AT files): the command or URC the vector belongs to (`+CREG?`, `+CMTI`, …).
- `codec`: our own annotation, not taken from the source. It records what serial-codec
  must do with the vector:
  - in `at_replies.json`, a list of exchanges `{command, raw, matched, body, urc,
    solicited_prefixes?, completion?}`. `raw` is the bytes as the modem sends them: the
    source sample is wrapped in echo, `\r\n` framing and `OK` when the source only had
    the body. `matched` is the `ExchangeMatch` (`null` means it never completes). `body`
    and `urc` are the expected `solicited_body` and `urc_lines`. An `ATE0` variant
    without the echo passes the command name as `solicited_prefixes`.
  - in `at_urc.json`, `{lines}`: the URC split into its lines (two lines for `+CDS:
    <len>` + PDU).
- `known_disagreement` (on a few entries): the source asserts vendor-specific
  behaviour that serial-codec deliberately doesn't follow (it implements only the
  V.250 / 27.007 / 27.005 behaviour). In `at_replies.json` it explains the `codec`
  expectation; the `urc/` tests require such entries to still disagree, so a change
  in behaviour is noticed.

## Privacy

Phone numbers and IMEIs flagged as real-looking were replaced with `+1000000000N` and
`000000000000000`. Flagged vectors whose numbers also sit inside PDU hex or a
field-parsing format were left out, or re-encoded with a fake number of the same length
(the ModemManager `+CDS` status report, recipient now `012345678`). The remaining numbers are the projects' own fakes
(`+0123456789`, `…123456…`, `+11111111112`).

## Not included

Advanced-option CMUX vectors (0x7E framing) are in `cmux.json`, and their tests are
`#[ignore]`d. Zephyr session tests (retries, power save, flow-control state) and
MTU-64-specific transmit sizes are left out. SMS PDU, GSM 7-bit, USSD and typed URC /
response vectors live in `sms/`, `ussd/` and `urc/`, each with its own README.
