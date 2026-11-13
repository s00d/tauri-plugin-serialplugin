//! Detect when an in-flight exchange response is complete.

use crate::at::parse::{AtCompletion, ExchangeMatch};
use crate::events::ExchangeCompletionMode;
use crate::exchange::options::{default_terminators, matches_terminators, ResolvedExchangeOptions};

/// Completion state of one exchange, fed chunk by chunk: each byte is looked at a
/// bounded number of times, so a long reply costs linear time.
#[derive(Debug)]
pub(crate) enum Completion {
    At(AtCompletion),
    Substring {
        terminators: Vec<Vec<u8>>,
        longest: usize,
    },
}

impl Completion {
    pub(crate) fn new(options: &ResolvedExchangeOptions) -> Self {
        match options.completion_mode {
            ExchangeCompletionMode::AtFinalLine => {
                Completion::At(AtCompletion::new(false, options.result_format))
            }
            ExchangeCompletionMode::AtIntermediate => {
                Completion::At(AtCompletion::new(true, options.result_format))
            }
            ExchangeCompletionMode::Substring => Completion::Substring {
                longest: options.terminators.iter().map(Vec::len).max().unwrap_or(0),
                terminators: options.terminators.clone(),
            },
        }
    }

    /// `buf` is everything received so far; its last `new` bytes just arrived.
    pub(crate) fn push(&mut self, buf: &[u8], new: usize) -> Option<ExchangeMatch> {
        let new = new.min(buf.len());
        match self {
            Completion::At(c) => c.push(&buf[buf.len() - new..]),
            Completion::Substring {
                terminators,
                longest,
            } => {
                // A terminator ending in the new bytes starts at most `longest - 1` before them.
                let from = (buf.len() - new).saturating_sub(longest.saturating_sub(1));
                matches_terminators(&buf[from..], terminators).then(|| ExchangeMatch::Substring {
                    term: String::from_utf8_lossy(default_terminators()[0].as_slice()).into_owned(),
                })
            }
        }
    }
}

/// One-shot check over a whole buffer.
#[cfg(test)]
pub(crate) fn check_exchange_complete(
    buf: &[u8],
    options: &ResolvedExchangeOptions,
) -> Option<ExchangeMatch> {
    Completion::new(options).push(buf, buf.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{AtResultFormat, RxPrepareMode};

    fn options(mode: ExchangeCompletionMode, terminators: Vec<Vec<u8>>) -> ResolvedExchangeOptions {
        ResolvedExchangeOptions {
            timeout_ms: 1000,
            max_bytes: 4096,
            terminators,
            idle_ms: None,
            rx_prepare: RxPrepareMode::None,
            drain_idle_ms: 50,
            drain_max_ms: 200,
            completion_mode: mode,
            result_format: AtResultFormat::Verbose,
            command: Some("AT".into()),
            solicited_prefixes: vec![],
        }
    }

    /// Feed `reply` one byte at a time, as a slow link delivers it.
    fn byte_by_byte(c: &mut Completion, reply: &[u8]) -> Vec<Option<ExchangeMatch>> {
        (1..=reply.len()).map(|n| c.push(&reply[..n], 1)).collect()
    }

    /// Each push looks at exactly the new bytes: the AT parser sees every byte once.
    #[test]
    fn at_completion_sees_each_new_byte() {
        let mut c = Completion::new(&options(ExchangeCompletionMode::AtFinalLine, vec![]));
        let seen = byte_by_byte(&mut c, b"AT\r\r\nOK\r\n");
        assert!(
            seen[..seen.len() - 1].iter().all(Option::is_none),
            "{seen:?}"
        );
        assert!(
            matches!(seen.last(), Some(Some(ExchangeMatch::Ok))),
            "{seen:?}"
        );
    }

    /// The substring window reaches back `longest - 1` bytes before the new ones: a
    /// terminator ending in the new bytes is found, one wholly in old bytes is not
    /// looked at again.
    #[test]
    fn substring_window_covers_a_terminator_ending_in_the_new_bytes() {
        let opts = options(ExchangeCompletionMode::Substring, vec![b"OK\r\n".to_vec()]);
        let mut c = Completion::new(&opts);
        let seen = byte_by_byte(&mut c, b"OK\r\n");
        assert!(seen[..3].iter().all(Option::is_none), "{seen:?}");
        assert!(seen[3].is_some(), "{seen:?}");
        let mut c = Completion::new(&opts);
        assert!(c.push(b"OK\r\nzz", 2).is_none());
        assert!(c.push(b"zzOK\r\n", 6).is_some());
    }
}
