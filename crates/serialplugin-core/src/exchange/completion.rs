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
