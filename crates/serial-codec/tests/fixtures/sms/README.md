# SMS fixtures

Test vectors (inputs and the values their source asserts) extracted from other projects'
test suites. They are **data only**: no code, comments or test structure was copied from
GPL/LGPL projects. Every entry keeps `source` (`project/path:line`) and `license`; `id` is
ours (`dec-NNN`, `enc-NNN`, `g7-NNN`, where NNN is the index in the original extraction).

| file | content | used by |
|---|---|---|
| `pdu_decode.json` | PDU and field decoding, multipart assembly (99 entries) | `tests/sms_vectors.rs` |
| `pdu_encode.json` | SMS-SUBMIT and field encoding (108 entries) | `tests/sms_vectors.rs` |
| `gsm7.json` | GSM 7-bit / national tables / packing / UCS2 (126 entries) | `tests/gsm7_vectors.rs` |
| `gateway_*.json` | the owner's `gateway-soft-rs` `gs-pdu` corpus | `tests/sms_vectors.rs` |

## Sources and licenses

| project | license | entries (decode / encode / gsm7) |
|---|---|---|
| AOSP telephony (`GsmSmsTest`, `GsmAlphabetTest`, `SmsMessageBodyTest`, …) | Apache-2.0 | 16 / 1 / 20 |
| chan_dongle (`test/gen.c`, `test/parse.c`) | GPL-2.0-only | – / 1 / 1 |
| gammu (`sms-pdu-security.c`, `at-charset.c`, `tests/at-sms*/*.dump`) | GPL-2.0-only | 7 / 33 / 2 |
| libosmocore (`tests/sms/sms_test.c`) | GPL-2.0-or-later | – / 6 / 20 |
| ModemManager (`test-sms-part-3gpp.c`, `test-sms-list.c`, `test-charsets.c`) | GPL-2.0-or-later | 14 / 4 / 16 |
| oFono (`unit/test-sms.c`, `unit/test-util.c`) | GPL-2.0-only | 8 / 10 / 17 |
| python-gsmmodem (`test/test_pdu.py`, `test/test_modem.py`) | LGPL-3.0-or-later | 18 / 28 / 12 |
| smspdudecoder (`tests/`, doctests in `smspdudecoder/`) | MIT | 17 / 8 / 21 |
| warthog618/sms (`encoding/*/…_test.go`, `collect_test.go`) | MIT | 19 / 17 / 17 |

`gateway_*.json` are copied from `gateway-soft-rs/crates/gs-pdu/tests/fixtures/`
(`pdu_golden_external.json`, `pdu_multipart.json`, `pdu_codec_golden.json`,
`bit8_external.json`, `rejected.json`); they in turn cite warthog618/sms (MIT), the
python-gsmmodem tests, morgverd/sms-pdu, fazalrasel/sms-pdu-decoder, rfc.nop.hu, diafaan.com
and a Stack Overflow question as the origin of each PDU. `legacy_ucs2_deliver_kz` was left
out: its message text contains a subscriber's phone number. `pdu_golden_generated.json` and
fixtures with IMEI/ICCID/MSISDN were not used.

## Privacy

No subscriber data: realistic phone numbers were replaced by same-length fakes
(`1000000001N`, `10000000000NN`, national `00000000NN`) in the input, in the semi-octets
of the PDU hex and in the expected values, so every length field stays valid and nothing
else changes. The original numbers are not recorded here.
Kept: the projects' pattern and fictional numbers (`…1234567`, `…555…`,
`+0123456789`, `+35010101010`, `+420800123456`). The gammu `tests/at-sms/*.dump` replies
(dec-018 … dec-031, "collected from random Internet postings", private messages
included) were dropped: they asserted only "decodes or fails", which dec-032 … dec-035
cover.

## Not copied

- **PII**: entries the extraction tagged `pii_phone` (19 decode, 15 encode), and the gammu
  `tests/at-sms` dumps (see Privacy).
- **Out of scope** (not a 23.038/23.040 PDU concern, or a library-internal API):
  libosmocore GSM 04.08 BCD LV and `osmo_bcd2str` (16 decode, 6 encode); warthog BCD helper,
  `decode_segments` (8-bit UD shown as text), TPDU struct encoders for non-SUBMIT types,
  chunk helpers, `smscounter`, SMS-DELIVER encoder, internal table sizes, custom charset
  injection (1 + 1 decode, 16 encode, 4 gsm7); python-gsmmodem type errors, naive datetimes
  and `discardInvalid` (2 encode, 1 gsm7); smspdudecoder enum validation and `reverse_octets`
  (3 encode, 1 gsm7); SIM EF/alpha-identifier coding (TS 31.102), EUC-KR ADN, AT `+CSCS`
  8859/CP437/UTF8 conversion (AOSP 2, ModemManager 6, oFono 5, gammu 5 gsm7).

## Known disagreements

Each is listed in the test file (`DISAGREE_*`), and the test fails if one starts passing.

| id | reason |
|---|---|
| dec-009, dec-010, dec-011 | CPHS voicemail indication carried in the OA (CPHS 4.2 §B.4.2) is not a 3GPP field; only DCS MWI groups are decoded |
| dec-014, dec-015 | Android's email-gateway split (23.040 §3.8) is application logic |
| dec-034, dec-035 | gammu accepts a TP-UDL that points past the data; we reject it (23.040 §9.2.3.16) |
| dec-038 | gammu's multipart hardening rejects a UDH that is valid per 23.040 §9.2.3.24.1 (ref 0xCE, part 1/1) |
| dec-080, enc-061, enc-062, enc-063 | oFono re-encodes SMS-DELIVER byte-exact; this crate only encodes SMS-SUBMIT (decode and lengths are checked) |
| dec-107 | python-gsmmodem prints semi-octet 1011 as `b`; 23.040 §9.1.2.3 maps it to `#` |
| dec-112, dec-113 | smspdudecoder rejects TOA 0x00 and NPI 2; we accept any TOA octet (receiver tolerance) |
| dec-114 | UCS2 user data shorter than TP-UDL (17 of 68 octets): rejected, smspdudecoder decodes with a warning |
| dec-132, g7-141…g7-144 | Indic national tables (Bengali, Kannada, Punjabi, Urdu — NLI 4–13) are not included |
| dec-149 (partial) | warthog collector: passes, except that seq 0 / seq > total is not an error — 23.040 §9.2.3.24.1 says ignore the IE, so the part is delivered as a single message; a duplicate part replaces the earlier one silently; SMS-DELIVER-REPORT steps are skipped |
| enc-002 | chan_dongle fills 152 septets per concatenated part; with a 6-octet UDH the maximum is 153 (we use 153) |
| enc-043 | alphanumeric address length: libosmocore writes octets×2 (14); 23.040 §9.1.2.5 counts useful semi-octets (13), as warthog and python-gsmmodem do |
| enc-044 | libosmocore truncates a 20-character alphanumeric address to its buffer; we reject more than 11 characters (20 semi-octets) |
| enc-090 | the fixture says `rejectDuplicates=true` but its PDU has TP-SRR set and TP-RD clear (first octet 0x31); the extracted argument name looks mislabelled |
| enc-094, enc-095, enc-098, enc-099 | python-gsmmodem `divideText*` always splits at the concatenated size (153 septets / 67 UCS2); 154 septets or 68 characters still fit one SMS (160 / 70) |
| enc-134 | an enhanced TP-VP is passed through raw, without 23.040 §9.2.3.12.3 validation |
| g7-012 | Android maps septet value 128 to a space; septets are 7-bit here and bit 7 is ignored |
| g7-021 | Android `use7bitOnly` (unmappable → space) has no equivalent |
| g7-112, g7-115 | smspdudecoder keeps the CR it adds after a CR on an octet boundary; we drop it (23.038 §6.1.2.3.1: CR CR ≡ CR) |
| `warthog_status_report_minimal` | truncated: TP-SCTS + TP-DT + TP-ST need 15 octets after TP-RA, 3 are present (23.040 §9.2.2.3) |
| `warthog_pdumode_roundtrip` | synthetic bytes `01..09`: TP-UDL 9 with no user data |
| enc-126 | warthog marshals an empty address as `00 00`; `encode_address` rejects an empty destination |
| `bit8_external` ussd case | read with CBS DCS 4 as 8-bit; `+CUSD` carries a CBS DCS (27.007 §7.15) and 0x04 is GSM 7-bit (23.038 §5). The text is checked with CBS 8-bit (0x44) |

Interpretation notes (not disagreements): gammu "Sent" entries are stored as SMS-DELIVER and
are checked by decoding; gammu SMS-SUBMIT entries are compared byte-exact when gammu did not
truncate or transliterate the text. python-gsmmodem sends numbers without `+` as national
(TOA 0xA1), so the test sets that TOA. ModemManager counts partial multipart messages.
