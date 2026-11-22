//! Model: the concatenated-SMS reassembler.
//!
//! Parts arrive in any order, with duplicates, gaps, bad sequence numbers and floods of other
//! messages; expiry and eviction come in between. After every step the reassembler and a
//! minimal reference agree on what completed (exactly once) and what is held.

use crate::common::*;
use serial_codec::sms::concat::{Fragment, Key, Kind, Message, Reassembler, DEFAULT_MAX_PENDING};
use serial_codec::sms::gsm7::{self, Language, Tables, ESC};
use serial_codec::sms::pdu::Alphabet;
use serial_codec::sms::ucs2;
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
enum Act {
    Push {
        key: usize,
        seq: u8,
        frag: usize,
        now: u64,
    },
    Expire {
        cutoff: u64,
    },
    /// First parts of `n` other long messages.
    Flood {
        n: u16,
        now: u64,
    },
}

fn keys() -> Vec<Key> {
    let k = |kind, address: &str, reference, ref_16bit, total| Key {
        kind,
        address: address.into(),
        reference,
        ref_16bit,
        total,
    };
    vec![
        k(Kind::Deliver, "+1", 1, false, 3),
        k(Kind::Submit, "+1", 1, false, 3),
        k(Kind::Deliver, "+1", 1, true, 3),
        k(Kind::Deliver, "+2", 7, false, 2),
        k(Kind::StatusReport, "+2", 7, false, 1),
    ]
}

fn frags() -> Vec<Fragment> {
    let f = |alphabet, tables, payload: &[u8]| Fragment {
        alphabet,
        tables,
        payload: payload.to_vec(),
    };
    let tr = Tables::new(Language::Turkish, Language::Default);
    vec![
        f(Alphabet::Gsm7, Tables::DEFAULT, &[0x41, 0x42]),
        f(Alphabet::Gsm7, Tables::DEFAULT, &[ESC, 0x65]),
        f(Alphabet::Gsm7, tr, &[0x07, 0x41]),
        f(Alphabet::Ucs2, Tables::DEFAULT, &[0x04, 0x16]),
        // An emoji split between parts by the sender.
        f(Alphabet::Ucs2, Tables::DEFAULT, &[0xD8, 0x3D]),
        f(Alphabet::Ucs2, Tables::DEFAULT, &[0xDE, 0x01]),
        f(Alphabet::Bit8, Tables::DEFAULT, &[1, 2, 3]),
    ]
}

fn act() -> impl Strategy<Value = Act> {
    prop_oneof![
        8 => (0usize..5, 0u8..5, 0usize..7, 0u64..20)
            .prop_map(|(key, seq, frag, now)| Act::Push { key, seq, frag, now }),
        1 => (0u64..22).prop_map(|cutoff| Act::Expire { cutoff }),
        1 => (1u16..100, 0u64..20).prop_map(|(n, now)| Act::Flood { n, now }),
    ]
}

/// The documented reassembler: parts by seq (a repeat replaces), at most `max`
/// incomplete messages, a new one evicts the least recently touched (newest part's
/// timestamp, then arrival).
struct Model {
    max: usize,
    tick: u64,
    pending: Vec<(Key, BTreeMap<u8, Fragment>, u64, u64)>,
}

impl Model {
    fn push(&mut self, key: Key, seq: u8, frag: Fragment, now: u64) -> Option<Message> {
        if seq == 0 || seq > key.total {
            return None;
        }
        self.tick += 1;
        if !self.pending.iter().any(|p| p.0 == key) && self.pending.len() >= self.max {
            let oldest = (0..self.pending.len())
                .min_by_key(|&i| (self.pending[i].2, self.pending[i].3))
                .unwrap();
            self.pending.remove(oldest);
        }
        let i = match self.pending.iter().position(|p| p.0 == key) {
            Some(i) => i,
            None => {
                self.pending.push((key.clone(), BTreeMap::new(), now, 0));
                self.pending.len() - 1
            }
        };
        let p = &mut self.pending[i];
        p.1.insert(seq, frag);
        p.2 = p.2.max(now);
        p.3 = self.tick;
        if p.1.len() < usize::from(key.total) {
            return None;
        }
        let parts: Vec<Fragment> = self.pending.remove(i).1.into_values().collect();
        let decode = |f: &Fragment, bytes: &[u8]| match f.alphabet {
            Alphabet::Gsm7 => Some(gsm7::decode(bytes, f.tables)),
            Alphabet::Ucs2 => Some(ucs2::decode(bytes)),
            Alphabet::Bit8 => None,
        };
        let payload: Vec<u8> = parts.iter().flat_map(|f| f.payload.clone()).collect();
        let uniform = parts
            .iter()
            .all(|f| f.alphabet == parts[0].alphabet && f.tables == parts[0].tables);
        let text = if uniform {
            decode(&parts[0], &payload)
        } else {
            parts.iter().map(|f| decode(f, &f.payload)).collect()
        };
        Some(Message {
            key: Some(key),
            alphabet: parts[0].alphabet,
            payload,
            text,
            parts: parts.len(),
        })
    }

    fn expire(&mut self, cutoff: u64) -> Vec<Key> {
        let (old, keep): (Vec<_>, Vec<_>) = self.pending.drain(..).partition(|p| p.2 < cutoff);
        self.pending = keep;
        old.into_iter().map(|p| p.0).collect()
    }
}

fn sorted(mut v: Vec<Key>) -> Vec<String> {
    let mut s: Vec<String> = v.drain(..).map(|k| format!("{k:?}")).collect();
    s.sort();
    s
}

/// The first part of the `r`-th message of a flood.
fn flood(r: u16) -> Key {
    Key {
        kind: Kind::Deliver,
        address: "+9".into(),
        reference: r,
        ref_16bit: true,
        total: 255,
    }
}

/// What one step must produce.
#[derive(Debug, PartialEq)]
enum Outcome {
    Pushed(Option<Message>),
    Expired(Vec<String>),
    /// How many messages completed.
    Flooded(usize),
}

impl Model {
    /// Expected outcome; `None` when the step is not observable now (expiry with
    /// nothing held).
    fn step(&mut self, a: &Act, keys: &[Key], frags: &[Fragment]) -> Option<Outcome> {
        match *a {
            Act::Push {
                key,
                seq,
                frag,
                now,
            } => Some(Outcome::Pushed(self.push(
                keys[key].clone(),
                seq,
                frags[frag].clone(),
                now,
            ))),
            Act::Expire { .. } if self.pending.is_empty() => None,
            Act::Expire { cutoff } => Some(Outcome::Expired(sorted(self.expire(cutoff)))),
            Act::Flood { n, now } => {
                let parts =
                    (0..n).filter(|&r| self.push(flood(r), 1, frags[0].clone(), now).is_some());
                Some(Outcome::Flooded(parts.count()))
            }
        }
    }
}

fn apply(real: &mut Reassembler, a: &Act, keys: &[Key], frags: &[Fragment]) -> Outcome {
    match *a {
        Act::Push {
            key,
            seq,
            frag,
            now,
        } => Outcome::Pushed(real.push(keys[key].clone(), seq, frags[frag].clone(), now)),
        Act::Expire { cutoff } => Outcome::Expired(sorted(real.expire_before(cutoff))),
        Act::Flood { n, now } => {
            let parts = (0..n).filter(|&r| real.push(flood(r), 1, frags[0].clone(), now).is_some());
            Outcome::Flooded(parts.count())
        }
    }
}

#[test]
fn reassembler_matches_its_model() {
    // `None`: the default cap.
    let cap = proptest::option::weighted(0.7, 0usize..5);
    run_model(512, (cap, vec(act(), 0..60)), |(max, acts)| {
        let (keys, frags) = (keys(), frags());
        let mut real = max.map_or_else(Reassembler::new, Reassembler::with_max_pending);
        let cap = max.unwrap_or(DEFAULT_MAX_PENDING).max(1);
        let mut model = Model {
            max: cap,
            tick: 0,
            pending: Vec::new(),
        };
        prop_assert_eq!(real.max_pending(), cap);
        for a in &acts {
            let Some(want) = model.step(a, &keys, &frags) else {
                continue;
            };
            prop_assert_eq!(apply(&mut real, a, &keys, &frags), want, "step {:?}", a);
            prop_assert_eq!(real.pending(), model.pending.len());
            prop_assert!(real.pending() <= cap);
        }
        Ok(())
    });
}
