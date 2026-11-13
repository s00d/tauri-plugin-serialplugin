//! Model tests for the framers driven by time: reads arrive with random gaps, polls and
//! end of stream come in between, and after every step the framer's frames (and its
//! deadline) equal a minimal reference model's.

use crate::common::*;
use serial_codec::framing::FramerConfig;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
enum Act {
    /// A read `gap` ms after the previous step.
    Push {
        gap: u64,
        data: Vec<u8>,
    },
    Poll {
        gap: u64,
    },
    Flush,
}

fn act(max_gap: u64) -> impl Strategy<Value = Act> {
    prop_oneof![
        6 => (0..max_gap, vec(prop_oneof![0u8..4, any::<u8>()], 0..8)).prop_map(|(gap, data)| Act::Push { gap, data }),
        2 => (0..max_gap).prop_map(|gap| Act::Poll { gap }),
        1 => Just(Act::Flush),
    ]
}

/// What a step must produce: frames (pushed, polled or flushed) and the next deadline.
type Outcome = (Vec<Vec<u8>>, Option<Instant>);

/// Inter-byte timeout: a frame ends once `interval` passes without data (noticed by a
/// poll or the next read), or when the buffer reaches `max`.
struct Silence {
    interval: Duration,
    max: usize,
    buf: Vec<u8>,
    last: Option<Instant>,
}

impl Silence {
    fn timeout(&mut self, now: Instant, out: &mut Vec<Vec<u8>>) {
        if self.last.is_some_and(|l| now >= l + self.interval) {
            self.last = None;
            if !self.buf.is_empty() {
                out.push(std::mem::take(&mut self.buf));
            }
        }
    }

    /// `None` when the step cannot be observed: a poll with nothing buffered.
    fn step(&mut self, a: &Act, now: Instant) -> Option<Outcome> {
        let mut out = Vec::new();
        match a {
            Act::Push { data, .. } => {
                self.timeout(now, &mut out);
                for &b in data {
                    self.buf.push(b);
                    if self.buf.len() >= self.max {
                        out.push(std::mem::take(&mut self.buf));
                    }
                }
                if !data.is_empty() {
                    self.last = Some(now);
                }
            }
            Act::Poll { .. } if self.buf.is_empty() => return None,
            Act::Poll { .. } => self.timeout(now, &mut out),
            Act::Flush => {
                self.last = None;
                out.extend((!self.buf.is_empty()).then(|| std::mem::take(&mut self.buf)));
            }
        }
        let deadline = self
            .last
            .filter(|_| !self.buf.is_empty())
            .map(|l| l + self.interval);
        Some((out, deadline))
    }
}

/// ccTalk: a frame is `buf[1] + 5` bytes; a gap over `max_delay` (0 = off) drops a
/// partial frame; end of stream drops it too.
struct CcTalk {
    max_delay: Duration,
    buf: Vec<u8>,
    last: Option<Instant>,
}

impl CcTalk {
    fn step(&mut self, a: &Act, now: Instant) -> Option<Outcome> {
        let mut out = Vec::new();
        match a {
            Act::Push { data, .. } => {
                if !self.max_delay.is_zero() {
                    if self.last.is_some_and(|l| now - l > self.max_delay) {
                        self.buf.clear();
                    }
                    self.last = Some(now);
                }
                self.buf.extend(data);
                while self.buf.len() >= 2 && self.buf.len() >= usize::from(self.buf[1]) + 5 {
                    let n = usize::from(self.buf[1]) + 5;
                    out.push(self.buf.drain(..n).collect());
                }
            }
            // ccTalk has no timer: a poll does nothing.
            Act::Poll { .. } => return None,
            Act::Flush => self.buf.clear(),
        }
        Some((out, None))
    }
}

fn apply(f: &mut dyn serial_codec::framing::Framer, a: &Act, now: Instant) -> Outcome {
    let mut out = Vec::new();
    match a {
        Act::Push { data, .. } => f.push(data, now, &mut out),
        Act::Poll { .. } => f.poll(now, &mut out),
        Act::Flush => out.extend(f.flush()),
    }
    (out, f.deadline())
}

fn gap(a: &Act) -> u64 {
    match a {
        Act::Push { gap, .. } | Act::Poll { gap } => *gap,
        Act::Flush => 0,
    }
}

#[test]
fn inter_byte_timeout_matches_model() {
    run_model(
        256,
        (1u64..40, 1usize..12, vec(act(60), 0..40)),
        |(interval, max, acts)| {
            let cfg = FramerConfig::InterByteTimeout {
                interval_ms: interval,
                max_buffer_size: Some(max),
            };
            let mut real = cfg.build().unwrap();
            let mut model = Silence {
                interval: Duration::from_millis(interval),
                max,
                buf: Vec::new(),
                last: None,
            };
            let mut now = Instant::now();
            for a in &acts {
                now += Duration::from_millis(gap(a));
                let Some(want) = model.step(a, now) else {
                    continue;
                };
                prop_assert_eq!(apply(&mut *real, a, now), want, "step {:?}", a);
            }
            Ok(())
        },
    );
}

#[test]
fn cctalk_matches_model() {
    let delay = prop_oneof![Just(None), Just(Some(0u64)), (1u64..60).prop_map(Some)];
    run_model(256, (delay, vec(act(80), 0..40)), |(delay, acts)| {
        let mut real = FramerConfig::CcTalk {
            max_delay_ms: delay,
        }
        .build()
        .unwrap();
        let mut model = CcTalk {
            max_delay: Duration::from_millis(delay.unwrap_or(50)),
            buf: Vec::new(),
            last: None,
        };
        let mut now = Instant::now();
        for a in &acts {
            now += Duration::from_millis(gap(a));
            let Some(want) = model.step(a, now) else {
                continue;
            };
            prop_assert_eq!(apply(&mut *real, a, now), want, "step {:?}", a);
        }
        Ok(())
    });
}
