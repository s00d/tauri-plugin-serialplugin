# USSD fixtures

`ussd.json` (51 entries, used by `tests/ussd_vectors.rs`): test vectors extracted from other
projects' test suites, **data only** (no code, comments or test structure copied). Each entry
keeps `source` (`project/path:line`) and `license`; `id` is ours (`us-NNN`, the index in the
original extraction).

| project | license | entries | what |
|---|---|---|---|
| chan_dongle (`test/parse.c`) | GPL-2.0-only | 3 | `+CUSD` line split into `<m>`, raw `<str>`, `<dcs>` |
| gammu (`tests/at-ussd/*.dump`) | GPL-2.0-only | 8 | real modem `+CUSD` replies (quoted/unquoted, packed hex, UCS2 hex, DCS 15/17/72) that must decode to text |
| libosmocore (`tests/ussd/ussd_test.c`) | GPL-2.0-or-later | 7 | 7-bit USSD packing with the 23.038 §6.1.2.3.1 CR rule |
| python-gsmmodem (`test/test_modem.py`) | LGPL-3.0-or-later | 13 | `+CUSD` session replies: message text and `<m>` session state |
| warthog618/sms (`encoding/gsm7/7bit_test.go`) | MIT | 20 | USSD/CBS pack and unpack with fill bits |

## Not copied

- oFono `unit/test-common.c` (32): 22.030 MMI/SS control-string classification, which is
  not `+CUSD` parsing or USSD coding.
- libosmocore GSM 04.80 facility IE / MAP SS ASN.1 decoding and encoding (9): radio-layer
  encoding, not what a modem returns over AT.

## Known disagreements

| id | reason |
|---|---|
| us-014 | libosmocore decodes `"0123456\r"` as `"0123456\r\r"` (it keeps the CR added after a CR on an octet boundary); we drop that CR (23.038 §6.1.2.3.1: CR CR is equivalent to CR). The packed bytes match |

Notes: python-gsmmodem session tests feed several lines; the check is that the `+CUSD`
line carrying the expected message parses to it and its `<m>` gives the session state
(session logic itself is the caller's). gammu entries assert only that the text is decoded
(not empty, not left as hex): with `at_charset` UCS2 through `text_as(Cscs::Ucs2)`, the
Huawei E160 replies (packed septets in hex under DCS 15, a firmware quirk, not 27.007)
through `text_packed_hex()`, the rest through `text()`.
