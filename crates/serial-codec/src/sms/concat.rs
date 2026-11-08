//! Reassembly of concatenated SMS (23.040 §9.2.3.24.1/.8).
//!
//! Parts are keyed by (TPDU type, address, reference and its width, total) so a
//! DELIVER and a SUBMIT sharing a reference never merge, nor do an 8-bit and a 16-bit
//! reference with the same value. Payloads are joined before decoding: GSM 7-bit
//! septets, UCS2 octets (a surrogate pair split across parts decodes correctly).
//!
//! Memory is bounded: at most [`Reassembler::max_pending`] incomplete messages are held
//! (default [`DEFAULT_MAX_PENDING`]); a new one beyond that evicts the oldest (by the
//! newest part's timestamp, then arrival). Part slots are allocated as parts arrive. No
//! timers: pass a timestamp of your choice to [`Reassembler::push`] and call
//! [`Reassembler::expire_before`] periodically so stale parts do not wait for eviction.

use super::gsm7::{self, Tables};
use super::pdu::{Alphabet, Tpdu};
use super::ucs2;
use std::collections::{BTreeMap, HashMap};

/// Incomplete messages a [`Reassembler::new`] holds before evicting the oldest.
pub const DEFAULT_MAX_PENDING: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Deliver,
    Submit,
    StatusReport,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Key {
    pub kind: Kind,
    /// Originator / destination / recipient as displayed (`+` for international).
    pub address: String,
    pub reference: u16,
    /// IEI 0x08 (16-bit reference) rather than 0x00: a different reference space.
    pub ref_16bit: bool,
    pub total: u8,
}

/// One part's user data, UDH removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fragment {
    pub alphabet: Alphabet,
    pub tables: Tables,
    /// Septets (GSM 7-bit) or octets.
    pub payload: Vec<u8>,
}

/// A complete message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// `None` for a message that was not concatenated.
    pub key: Option<Key>,
    pub alphabet: Alphabet,
    /// Joined payloads (septets or octets).
    pub payload: Vec<u8>,
    /// `None` for 8-bit data.
    pub text: Option<String>,
    pub parts: usize,
}

struct Pending {
    /// Parts received so far, by `seq`.
    parts: BTreeMap<u8, Fragment>,
    last_seen: u64,
    /// Arrival order of the newest part (tie-break for eviction).
    tick: u64,
}

pub struct Reassembler {
    pending: HashMap<Key, Pending>,
    max_pending: usize,
    tick: u64,
}

impl Default for Reassembler {
    fn default() -> Self {
        Self::with_max_pending(DEFAULT_MAX_PENDING)
    }
}

impl Reassembler {
    /// Holds up to [`DEFAULT_MAX_PENDING`] incomplete messages.
    pub fn new() -> Self {
        Self::default()
    }

    /// Holds up to `max` incomplete messages (at least 1).
    pub fn with_max_pending(max: usize) -> Self {
        Reassembler {
            pending: HashMap::new(),
            max_pending: max.max(1),
            tick: 0,
        }
    }

    /// The cap on incomplete messages.
    pub fn max_pending(&self) -> usize {
        self.max_pending
    }

    /// Add part `seq` (1-based) of `key`. Returns the message once every part is in.
    /// A repeated `seq` replaces the earlier copy; `seq` outside `1..=total` is ignored.
    /// A part of a new message when [`Self::max_pending`] are held evicts the oldest.
    pub fn push(&mut self, key: Key, seq: u8, fragment: Fragment, now: u64) -> Option<Message> {
        if seq == 0 || seq > key.total {
            return None;
        }
        self.tick += 1;
        if !self.pending.contains_key(&key) && self.pending.len() >= self.max_pending {
            // ponytail: O(max_pending) scan per new message; an ordered index if the cap
            // ever gets large.
            let oldest = self
                .pending
                .iter()
                .min_by_key(|(_, p)| (p.last_seen, p.tick))
                .map(|(k, _)| k.clone());
            if let Some(k) = oldest {
                self.pending.remove(&k);
            }
        }
        let entry = self.pending.entry(key.clone()).or_insert_with(|| Pending {
            parts: BTreeMap::new(),
            last_seen: now,
            tick: 0,
        });
        entry.parts.insert(seq, fragment);
        entry.last_seen = entry.last_seen.max(now);
        entry.tick = self.tick;
        if entry.parts.len() < usize::from(key.total) {
            return None;
        }
        let parts: Vec<Fragment> = self.pending.remove(&key)?.parts.into_values().collect();
        Some(join(Some(key), parts))
    }

    /// Feed a decoded TPDU. Messages without a valid concatenation IE complete at once.
    pub fn push_tpdu(&mut self, tpdu: &Tpdu, now: u64) -> Option<Message> {
        let ud = tpdu.user_data()?;
        let dcs = tpdu.dcs().copied().unwrap_or(super::pdu::Dcs::decode(0));
        let alphabet = if dcs.compressed {
            Alphabet::Bit8
        } else {
            dcs.alphabet
        };
        let fragment = Fragment {
            alphabet,
            tables: ud.udh.as_ref().map_or(Tables::DEFAULT, |u| u.tables()),
            payload: ud.payload.clone(),
        };
        let Some(c) = ud.udh.as_ref().and_then(|u| u.concat()) else {
            return Some(join(None, vec![fragment]));
        };
        let kind = match tpdu {
            Tpdu::Deliver(_) => Kind::Deliver,
            Tpdu::Submit(_) => Kind::Submit,
            Tpdu::StatusReport(_) => Kind::StatusReport,
        };
        let key = Key {
            kind,
            address: tpdu.address().to_string(),
            reference: c.reference,
            ref_16bit: c.ref_16bit,
            total: c.total,
        };
        self.push(key, c.seq, fragment, now)
    }

    /// Drop incomplete messages whose newest part arrived before `cutoff`; returns
    /// their keys.
    pub fn expire_before(&mut self, cutoff: u64) -> Vec<Key> {
        let old: Vec<Key> = self
            .pending
            .iter()
            .filter(|(_, p)| p.last_seen < cutoff)
            .map(|(k, _)| k.clone())
            .collect();
        for k in &old {
            self.pending.remove(k);
        }
        old
    }

    /// Number of incomplete messages held.
    pub fn pending(&self) -> usize {
        self.pending.len()
    }
}

fn join(key: Option<Key>, parts: Vec<Fragment>) -> Message {
    let first = parts[0].clone();
    let uniform = parts
        .iter()
        .all(|p| p.alphabet == first.alphabet && p.tables == first.tables);
    let payload: Vec<u8> = parts
        .iter()
        .flat_map(|p| p.payload.iter().copied())
        .collect();
    let decode = |alphabet, tables, bytes: &[u8]| match alphabet {
        Alphabet::Gsm7 => Some(gsm7::decode(bytes, tables)),
        Alphabet::Ucs2 => Some(ucs2::decode(bytes)),
        Alphabet::Bit8 => None,
    };
    let text = if uniform {
        decode(first.alphabet, first.tables, &payload)
    } else {
        parts
            .iter()
            .map(|p| decode(p.alphabet, p.tables, &p.payload))
            .collect()
    };
    Message {
        key,
        alphabet: first.alphabet,
        payload,
        text,
        parts: parts.len(),
    }
}
