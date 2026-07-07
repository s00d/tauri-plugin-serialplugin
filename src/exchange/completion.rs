//! Detect when an in-flight exchange response is complete.

use crate::at::parse::{at_final_line_complete, at_intermediate_line_complete, ExchangeMatch};
use crate::events::ExchangeCompletionMode;
use crate::exchange::options::{default_terminators, matches_terminators, ResolvedExchangeOptions};

pub(crate) fn check_exchange_complete(
    buf: &[u8],
    options: &ResolvedExchangeOptions,
) -> Option<ExchangeMatch> {
    match options.completion_mode {
        ExchangeCompletionMode::AtFinalLine => at_final_line_complete(buf, options.result_format),
        ExchangeCompletionMode::AtIntermediate => {
            at_intermediate_line_complete(buf, options.result_format)
        }
        ExchangeCompletionMode::Substring => {
            if matches_terminators(buf, &options.terminators) {
                Some(ExchangeMatch::Substring {
                    term: String::from_utf8_lossy(default_terminators()[0].as_slice()).into_owned(),
                })
            } else {
                None
            }
        }
    }
}
