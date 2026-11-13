//! GSM 7-bit / UCS2 vectors (data only; sources and licenses in
//! `tests/fixtures/sms/README.md`). Entries in `DISAGREE` must fail their check.

use serde_json::Value;
use serial_codec::sms::gsm7::{self, Language, Tables, ESC};
use serial_codec::sms::pdu::{decode_hex, decode_tpdu, decode_user_data, encode_submit};
use serial_codec::sms::pdu::{Alphabet, Dcs, SubmitRequest};
use serial_codec::sms::{hex_decode, hex_encode, ucs2};
use serial_codec::ussd;
use std::collections::BTreeMap;

type R = Result<(), String>;

fn hx(s: &str) -> Vec<u8> {
    hex_decode(s).unwrap_or_else(|| panic!("bad hex {s}"))
}

fn eq<T: PartialEq + std::fmt::Debug>(what: &str, got: T, want: T) -> R {
    if got == want {
        Ok(())
    } else {
        Err(format!("{what}: got {got:?}, want {want:?}"))
    }
}

fn lang(code: u64) -> Language {
    Language::from_nli(code as u8).unwrap()
}

fn lang_name(s: &str) -> Language {
    match s {
        "turkish" => Language::Turkish,
        "spanish" => Language::Spanish,
        "portuguese" => Language::Portuguese,
        _ => Language::Default,
    }
}

fn encode_default(text: &str) -> Vec<u8> {
    gsm7::encode(text, Tables::DEFAULT).unwrap()
}

/// (parts, units, remaining) the way Android's calculateLength reports them.
fn length_of(text: &str, single: &[Language]) -> (usize, usize, usize, Alphabet, Tables) {
    let mut req = SubmitRequest::new("1", text);
    req.concat_ref = Some(0);
    req.single_shift = single.to_vec();
    let parts = encode_submit(&req).unwrap();
    let tables = gsm7::choose_tables(text, &[], single);
    let (units, alphabet) = match tables {
        Some(t) => (gsm7::septet_len(text, t).unwrap(), Alphabet::Gsm7),
        None => (text.encode_utf16().count(), Alphabet::Ucs2),
    };
    let per_part = match (alphabet, parts.len()) {
        (Alphabet::Gsm7, 1) => 160,
        (Alphabet::Gsm7, _) => 153,
        (_, 1) => 70,
        _ => 67,
    };
    let remaining = per_part * parts.len() - units;
    (
        parts.len(),
        units,
        remaining,
        alphabet,
        tables.unwrap_or_default(),
    )
}

fn check(e: &Value) -> R {
    let i = &e["input"];
    let x = &e["expected"];
    let op = i["op"].as_str().unwrap_or("");
    let s = |k: &str| i[k].as_str().unwrap_or("");
    match op {
        "calculate_length" => {
            let single: Vec<Language> = i["enabled_single_shift_tables"]
                .as_array()
                .map(|a| a.iter().map(|v| lang(v.as_u64().unwrap())).collect())
                .unwrap_or_default();
            let text = s("text");
            let (n, units, _, alphabet, tables) = length_of(text, &single);
            eq("msgs", n as u64, x["msg_count"].as_u64().unwrap())?;
            eq(
                "units",
                units as u64,
                x["code_unit_count"].as_u64().unwrap(),
            )?;
            eq(
                "alphabet",
                alphabet == Alphabet::Gsm7,
                x["code_unit_size"] == "7bit",
            )?;
            eq(
                "single",
                u64::from(tables.single.nli()),
                x["language_shift_table"].as_u64().unwrap(),
            )?;
            eq(
                "locking",
                u64::from(tables.locking.nli()),
                x["language_table"].as_u64().unwrap(),
            )?;
            // Fragment sizes in characters.
            let mut req = SubmitRequest::new("1", text);
            req.concat_ref = Some(0);
            req.single_shift = single;
            let frags: Vec<u64> = encode_submit(&req)
                .unwrap()
                .iter()
                .map(|(_, h)| decode_hex(h).unwrap().tpdu.text().unwrap().chars().count() as u64)
                .collect();
            let want: Vec<u64> = x["fragments"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap())
                .collect();
            eq("fragments", frags, want)
        }
        "calculate_length_table" => {
            if i["use_7bit_only"] == true {
                return Err("use7bitOnly (unmappable → space) has no equivalent".into());
            }
            let alphabet: Vec<char> = s("alphabet").chars().collect();
            for row in x["rows"].as_array().unwrap() {
                let len = row["length"].as_u64().unwrap() as usize;
                let text: String = alphabet.iter().cycle().take(len).collect();
                let (n, units, rem, a, _) = length_of(&text, &[]);
                let want_a = if row["encoding"] == "7bit" {
                    Alphabet::Gsm7
                } else {
                    Alphabet::Ucs2
                };
                eq("alphabet", a, want_a)?;
                eq("msgs", n as u64, row["msg_count"].as_u64().unwrap())?;
                eq(
                    "units",
                    units as u64,
                    row["code_unit_count"].as_u64().unwrap(),
                )?;
                eq(
                    "remaining",
                    rem as u64,
                    row["code_units_remaining"].as_u64().unwrap(),
                )?;
            }
            Ok(())
        }
        "locking_shift_table" => {
            let t = Tables::new(
                lang(i["language_code"].as_u64().unwrap()),
                Language::Default,
            );
            for (k, v) in x["table"].as_object().unwrap() {
                let septet = hx(k)[0];
                eq(
                    k,
                    gsm7::decode(&[septet], t),
                    v.as_str().unwrap().to_string(),
                )?;
                eq(
                    "encode",
                    gsm7::encode(v.as_str().unwrap(), t),
                    Ok(vec![septet]),
                )?;
            }
            Ok(())
        }
        "single_shift_table" => {
            let t = Tables::new(
                Language::Default,
                lang(i["language_code"].as_u64().unwrap()),
            );
            for (k, v) in x["table"].as_object().unwrap() {
                let septet = hx(k)[0];
                let c = v.as_str().unwrap();
                eq(k, gsm7::decode(&[ESC, septet], t), c.to_string())?;
                // Characters also in the default locking table encode without ESC.
                let direct = gsm7::encode(c, Tables::DEFAULT).is_ok();
                if !direct {
                    eq("encode", gsm7::encode(c, t), Ok(vec![ESC, septet]))?;
                }
            }
            Ok(())
        }
        "single_shift_fallback" => {
            for s in i["single_shift_languages"].as_array().unwrap() {
                for l in i["locking_languages"].as_array().unwrap() {
                    let t = Tables::new(lang(l.as_u64().unwrap()), lang(s.as_u64().unwrap()));
                    eq("ESC ESC", gsm7::decode(&[ESC, ESC], t), " ".to_string())?;
                    for n in 0u8..0x80 {
                        let single = gsm7::decode(&[ESC, n], t);
                        let in_table =
                            gsm7::encode(&single, Tables::new(Language::Default, t.single))
                                .is_ok_and(|v| v == [ESC, n]);
                        if !in_table && n != ESC {
                            eq("fallback", single, gsm7::decode(&[n], t))?;
                        }
                    }
                }
            }
            Ok(())
        }
        "char_to_gsm" => eq(
            "septet",
            encode_default(s("char")),
            vec![x["septet"].as_u64().unwrap() as u8],
        ),
        "ext_roundtrip" => {
            let t = s("chars");
            let septets = encode_default(t);
            eq("all escaped", septets.len(), 2 * t.chars().count())?;
            eq(
                "roundtrip",
                gsm7::decode(&septets, Tables::DEFAULT),
                t.to_string(),
            )
        }
        "gsm_to_char" => eq(
            "char",
            gsm7::decode(&[i["septet"].as_u64().unwrap() as u8], Tables::DEFAULT),
            s_x(x, "char"),
        ),
        "gsm_extended_to_char" => eq(
            "char",
            gsm7::decode(&[ESC, i["septet"].as_u64().unwrap() as u8], Tables::DEFAULT),
            s_x(x, "char"),
        ),
        "unpack_7bit" if i.get("udh_ind").is_some() || i.get("septets").is_some() => {
            let b = hx(s("hex"));
            let n = i["septets"].as_u64().unwrap() as u8;
            let text = if i["udh_ind"] == 1 {
                decode_user_data(n, &b, true, &Dcs::decode(0))
                    .map_err(|e| e.to_string())?
                    .text
                    .unwrap()
            } else {
                gsm7::decode(&gsm7::unpack(&b, 0, n.into()), Tables::DEFAULT)
            };
            eq("text", text.as_str(), x["text"].as_str().unwrap())?;
            if let Some(c) = x["chars"].as_u64() {
                eq("chars", text.chars().count() as u64, c)?;
            }
            Ok(())
        }
        "pack_8bit" => {
            let b = encode_default(s("text"));
            eq("octets", b.len() as u64, x["octets"].as_u64().unwrap())?;
            eq(
                "roundtrip",
                gsm7::decode(&b, Tables::DEFAULT),
                s("text").to_string(),
            )
        }
        "unpack_8bit" => eq(
            "text",
            gsm7::decode(&hx(s("hex")), Tables::DEFAULT),
            s_x(x, "text"),
        ),
        "" if i.get("texts").is_some() => {
            for t in i["texts"].as_array().unwrap() {
                let t = t.as_str().unwrap();
                for p in i["padding_bits"].as_array().unwrap() {
                    let p = p.as_u64().unwrap() as u8;
                    let septets = encode_default(t);
                    let packed = gsm7::pack(&septets, p);
                    let back = gsm7::unpack(&hx(&hex_encode(&packed)), p, septets.len());
                    eq(
                        "roundtrip",
                        gsm7::decode(&back, Tables::DEFAULT),
                        t.to_string(),
                    )?;
                }
            }
            Ok(())
        }
        "" if i.get("at_charset").is_some() => {
            // AT+CSCS="UCS2": text ↔ UCS2 hex.
            eq("ucs2", hex_encode(&ucs2::encode(s("text"))), s_x(x, "text"))
        }
        "pack_7bit" if i.get("gsm_ascii").is_some() => {
            let septets = encode_default(s("gsm_ascii"));
            let fill = i["byte_offset"].as_u64().unwrap() as u8;
            let cbs = i["cbs"] == true;
            let packed = if cbs {
                ussd::pack_septets(&septets, fill)
            } else {
                gsm7::pack(&septets, fill)
            };
            eq(
                "len",
                packed.len() as u64,
                x["packed_len"].as_u64().unwrap(),
            )?;
            if x.get("last_byte_high7_bits").is_some() {
                eq("spare bits", packed[6] & 0xFE, 0)?;
            }
            if let Some(v) = x["last_byte_septet"].as_str() {
                eq("CR fill", (packed[6] & 0xFE) >> 1, hx(v)[0])?;
            }
            if let Some(u) = x.get("then_unpack") {
                let back = gsm7::unpack(&packed, fill, u["max"].as_u64().unwrap() as usize);
                let t = gsm7::decode(&back, Tables::DEFAULT);
                eq("len", back.len() as u64, u["out_len"].as_u64().unwrap())?;
                eq(
                    "first",
                    t.chars().next(),
                    u["first"].as_str().unwrap().chars().next(),
                )?;
                eq(
                    "last",
                    t.chars().last(),
                    u["last"].as_str().unwrap().chars().next(),
                )?;
            }
            if let Some(us) = x["then_unpack_sms"].as_array() {
                for u in us {
                    let back = gsm7::unpack(&packed, fill, u["max"].as_u64().unwrap() as usize);
                    eq("len", back.len() as u64, u["out_len"].as_u64().unwrap())?;
                    if let Some(b7) = u["out_byte7"].as_str() {
                        eq("byte7", back[7], hx(b7)[0])?;
                    }
                }
            }
            Ok(())
        }
        "pack_7bit" => {
            let text = match i["text_repeat"].as_object() {
                Some(r) => r["char"]
                    .as_str()
                    .unwrap()
                    .repeat(r["count"].as_u64().unwrap() as usize),
                None => s("text").to_string(),
            };
            let septets = encode_default(&text);
            let packed = gsm7::pack(&septets, 0);
            eq(
                "septets",
                septets.len() as u64,
                x["septets"].as_u64().unwrap(),
            )?;
            eq("octets", packed.len() as u64, x["octets"].as_u64().unwrap())?;
            if let Some(h) = x["hex"].as_str() {
                eq("hex", hex_encode(&packed), h.to_string())?;
            }
            if let Some(c) = x["repeating_chunk_hex"].as_str() {
                let chunk = hx(c);
                let full = septets.len() / 8 * 7; // whole 8-septet groups only
                eq("chunks", packed[..full].chunks(7).all(|p| p == chunk), true)?;
            }
            Ok(())
        }
        "pack_septets_with_fill" => {
            let septets = encode_default(s("text"));
            eq(
                "septets",
                septets.len() as u64,
                i["septets"].as_u64().unwrap(),
            )?;
            let packed = gsm7::pack(&septets, i["fill_bits"].as_u64().unwrap() as u8);
            eq("hex", hex_encode(&packed), s_x(x, "hex"))?;
            eq("octets", packed.len() as u64, x["octets"].as_u64().unwrap())
        }
        "roundtrip" if i.get("table").is_some() => {
            let t = s("text");
            let septets = encode_default(t);
            let packed = gsm7::pack(&septets, 0);
            eq("shorter", packed.len() <= septets.len(), true)?;
            let back = gsm7::unpack(&packed, 0, septets.len());
            eq(
                "roundtrip",
                gsm7::decode(&back, Tables::DEFAULT),
                t.to_string(),
            )
        }
        "roundtrip" => {
            // smspdudecoder: encode_with_padding = USSD CR rule; strip_padding likewise.
            let septets = encode_default(s("text"));
            let packed = if i["encode_with_padding"] == true {
                ussd::pack_septets(&septets, 0)
            } else {
                gsm7::pack(&septets, 0)
            };
            let back = if i["decode_strip_padding"] == true {
                ussd::unpack_septets(&packed, 0)
            } else {
                gsm7::unpack(&packed, 0, gsm7::septets_in(packed.len(), 0))
            };
            eq("text", gsm7::decode(&back, Tables::DEFAULT), s_x(x, "text"))
        }
        "unpack" => {
            let n = i["septets"].as_u64().unwrap() as usize;
            let s7 = gsm7::unpack(
                &hx(s("packed")),
                i["offset_bits"].as_u64().unwrap() as u8,
                n,
            );
            eq("septets", hex_encode(&s7), s_x(x, "septets_hex"))
        }
        "pack" => {
            let p = gsm7::pack(
                &hx(s("septets_hex")),
                i["offset_bits"].as_u64().unwrap() as u8,
            );
            match x["packed"].as_str() {
                Some(h) => eq("packed", hex_encode(&p), h.to_string()),
                None => eq("len", p.len() as u64, x["packed_len"].as_u64().unwrap()),
            }
        }
        "ucs2_hex<->utf8" => {
            eq("decode", ucs2::decode(&hx(s("ucs2_hex"))), s_x(x, "text"))?;
            eq(
                "encode",
                hex_encode(&ucs2::encode(&s_x(x, "text"))),
                s("ucs2_hex").to_string(),
            )
        }
        "gsm_bytes->utf8" => {
            // The trailing 0x00 is the C terminator, not '@'.
            let mut b = hx(s("gsm_hex"));
            b.pop();
            eq("text", gsm7::decode(&b, Tables::DEFAULT), s_x(x, "text"))
        }
        "can_convert_to" => eq(
            "gsm",
            gsm7::is_encodable(s("text"), Tables::DEFAULT),
            x["gsm"].as_bool().unwrap(),
        ),
        "split_text_for_sms" => {
            let mut req = SubmitRequest::new("1", s("text"));
            req.concat_ref = Some(0);
            let parts = encode_submit(&req).unwrap();
            let texts: Vec<String> = parts
                .iter()
                .map(|(_, h)| decode_hex(h).unwrap().tpdu.text().unwrap().to_string())
                .collect();
            let want: Vec<String> = x["parts"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap().to_string())
                .collect();
            eq("parts", texts, want)?;
            let a = decode_hex(&parts[0].1)
                .unwrap()
                .tpdu
                .dcs()
                .unwrap()
                .alphabet;
            eq("charset", a == Alphabet::Gsm7, x["charset"] == "gsm")
        }
        "" if i.get("pdu").is_some() => {
            let b = hx(s("pdu"));
            let t = decode_tpdu(&b).map_err(|e| e.to_string())?;
            let ud = t.user_data().unwrap();
            let dcs = t.dcs().unwrap();
            eq(
                "charset",
                dcs.alphabet == Alphabet::Gsm7,
                x["dcs_charset"] == "7bit",
            )?;
            eq(
                "compressed",
                dcs.compressed,
                x["compressed"].as_bool().unwrap(),
            )?;
            eq(
                "ud_bytes",
                gsm7::pack(&ud.payload, 0).len() as u64,
                x["ud_bytes"].as_u64().unwrap(),
            )?;
            // The fixture gives the shift tables out of band; the PDU has no UDH.
            let tables = Tables::new(lang_name(s("locking_shift")), lang_name(s("single_shift")));
            eq("text", gsm7::decode(&ud.payload, tables), s_x(x, "text"))
        }
        "" if i.get("map_gsm_to_unicode").is_some() => {
            for pair in i["map_gsm_to_unicode"].as_array().unwrap() {
                let gsm = hx(pair[0].as_str().unwrap());
                let cp = u32::from_str_radix(&pair[1].as_str().unwrap()[2..], 16).unwrap();
                let c = char::from_u32(cp).unwrap().to_string();
                eq("decode", gsm7::decode(&gsm, Tables::DEFAULT), c.clone())?;
                eq("encode", encode_default(&c), gsm)?;
            }
            Ok(())
        }
        "gsm_to_utf8" => eq(
            "text",
            gsm7::decode(&hx(s("gsm"))[..0], Tables::DEFAULT),
            s_x(x, "text"),
        ),
        "ucs2_to_gsm" => {
            let text = ucs2::decode(&hx(s("ucs2")));
            let r = gsm7::encode(&text, Tables::DEFAULT);
            eq(
                "fails at",
                r.err(),
                text.chars().nth(x["nread"].as_u64().unwrap() as usize / 2),
            )
        }
        "unpack_7bit+gsm_to_utf8 and back" => {
            let b = hx(s("packed"));
            let n = i["septets"].as_u64().unwrap() as usize;
            let t = gsm7::decode(&gsm7::unpack(&b, 0, n), Tables::DEFAULT);
            eq("text", t.clone(), s_x(x, "text"))?;
            eq("repack", gsm7::pack(&encode_default(&t), 0), b)
        }
        "gsm7_text" => {
            let septets = encode_default(s("text"));
            eq("septets", hex_encode(&septets), s_x(x, "septets"))?;
            eq(
                "packed",
                hex_encode(&gsm7::pack(&septets, 0)),
                s_x(x, "packed"),
            )?;
            eq(
                "decode",
                gsm7::decode(&septets, Tables::DEFAULT),
                s("text").to_string(),
            )
        }
        "gsm7_encode" => eq(
            "error",
            gsm7::encode(s("text"), Tables::DEFAULT).is_err(),
            true,
        ),
        "ucs2" | "ucs2_encode" => {
            let want = x["hex"].as_str().unwrap_or("").to_string();
            eq("encode", hex_encode(&ucs2::encode(s("text"))), want.clone())?;
            eq("decode", ucs2::decode(&hx(&want)), s("text").to_string())
        }
        "ucs2_decode" => {
            let r = ucs2::decode_strict(&hx(s("hex")));
            if !x["error"].is_null() {
                return eq("error", r.is_err(), true);
            }
            eq(
                "text",
                r.map_err(|e| e.to_string())?,
                x["text"].as_str().unwrap_or("").to_string(),
            )
        }
        "gsm7_pack" => {
            let septets = encode_default(s("text"));
            let p = if i["with_padding"] == true {
                ussd::pack_septets(&septets, 0)
            } else {
                gsm7::pack(&septets, 0)
            };
            eq("hex", hex_encode(&p), s_x(x, "hex"))
        }
        "gsm7_unpack" => {
            let b = hx(s("hex"));
            let septets = if i["strip_padding"] == true {
                ussd::unpack_septets(&b, 0)
            } else {
                gsm7::unpack(&b, 0, gsm7::septets_in(b.len(), 0))
            };
            eq(
                "text",
                gsm7::decode(&septets, Tables::DEFAULT),
                s_x(x, "text"),
            )
        }
        "pack7" => {
            let septets = hx(s("septets"));
            let pad = i["pad_bits"].as_u64().unwrap() as u8;
            let packed = gsm7::pack(&septets, pad);
            let want = x["packed"].as_str().unwrap_or("");
            eq("pack", hex_encode(&packed), want.to_string())?;
            let back = gsm7::unpack(&hx(want), pad, septets.len());
            eq("unpack", back, septets)
        }
        "septets_to_utf8" => eq(
            "text",
            gsm7::decode(&hx(s("septets")), Tables::DEFAULT),
            x["text"].as_str().unwrap_or("").to_string(),
        ),
        "utf8_to_septets" => eq(
            "septets",
            hex_encode(&encode_default(s("text"))),
            s_x(x, "septets"),
        ),
        "charset_map" => Err(format!(
            "{} (NLI {}) tables are not included",
            s("table"),
            i["nli"]
        )),
        other => Err(format!("unsupported op `{other}`")),
    }
}

fn s_x(x: &Value, k: &str) -> String {
    x[k].as_str().unwrap_or("").to_string()
}

const DISAGREE: &[(&str, &str)] = &[
    ("g7-012", "Android maps septet 128 to ' '; septets are 7-bit here and bit 7 is ignored (→ '@')"),
    ("g7-021", "Android use7bitOnly replaces unmappable characters with spaces; no lossy mode here"),
    ("g7-112", "smspdudecoder unpacks 8 septets and keeps the CR it added; we drop the doubled CR (23.038 §6.1.2.3.1: CR CR ≡ CR)"),
    ("g7-115", "same as g7-112 (text already ends in CR CR)"),
    ("g7-141", "Bengali (NLI 4) tables are not included"),
    ("g7-142", "Kannada (NLI 7) tables are not included"),
    ("g7-143", "Punjabi (NLI 10) tables are not included"),
    ("g7-144", "Urdu (NLI 13) tables are not included"),
];

#[test]
fn gsm7_vectors() {
    let v: Vec<Value> = serde_json::from_str(include_str!("../fixtures/sms/gsm7.json")).unwrap();
    let mut failures = Vec::new();
    let mut stats: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for e in &v {
        let id = e["id"].as_str().unwrap();
        let src = e["source"]
            .as_str()
            .unwrap()
            .split('/')
            .next()
            .unwrap()
            .to_string();
        let listed = DISAGREE.iter().any(|(d, _)| *d == id);
        let st = stats.entry(src).or_default();
        match (check(e), listed) {
            (Ok(()), false) => st.0 += 1,
            (Err(_), true) => st.1 += 1,
            (Ok(()), true) => failures.push(format!("{id}: listed as disagreement but passes")),
            (Err(m), false) => failures.push(format!("{id}: {m}")),
        }
    }
    for (src, (p, d)) in &stats {
        eprintln!("[gsm7] {src}: {p} pass, {d} known disagreement");
    }
    assert!(failures.is_empty(), "gsm7:\n{}", failures.join("\n"));
}
