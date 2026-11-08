# URC / response vectors

`at_urc.json` and `at_responses.json` are subsets of vectors harvested from other projects' test
assertions, keyed by command. Each entry keeps `source` (`project/path:line`), `license`, `input`,
`expected` and `note`. They are run by `tests/urc_vectors.rs`.

These files contain **data only** (modem lines and the decoded values those projects assert).
No code, comments or test structure was copied.

| project | license | used for |
|---|---|---|
| ModemManager (`src/tests/test-modem-helpers.c`) | GPL-2.0-or-later | +CREG/+CGREG/+CEREG/+C5GREG, +CGEV, +CLIP, +CCWA, +CDS, +CESQ, +CIND=?, +CLCC, +CMGL, +CMGR, +COPS, +CPIN?, +CPMS |
| chan_dongle (`test/parse.c`) | GPL-2.0-only | +CMTI, +CLCC, +CMGR, +COPS?, +CREG? |
| gammu (`tests/at-*`) | GPL-2.0-only | +CMGL, +CMGR, +CNMI=?, +CPIN, +CPMS=? |
| python-gsmmodem (`test/`) | LGPL-3.0-or-later | +CMTI, +CDSI, +CRING/RING + +CLIP, +CMGL, +CMGS, +COPS?, +CSQ |

Changes from the harvested data:
- Phone numbers that looked real were replaced with fakes (`+10000000001` ... `+10000000005`),
  including the entries tagged `pii_phone`.
- Dropped: a Zephyr `modem_chat` argv-splitter case (different semantics) and gammu's empty
  `AT+CREG?` reply (nothing to parse). Vendor-only commands (`^`, `+Q...`) were not taken.
- `known_disagreement` marks an entry this crate deliberately decodes differently, with the reason
  (vendor quirk or spec section). The test requires those to still differ.

ModemManager asserts its own enum names (`home`, `lte`, `nw_deact_pdp`, ...) and uses 0 for an
absent LAC/CI; the test maps this crate's types onto those names.
