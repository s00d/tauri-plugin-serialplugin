//! serial-codec behaviour tests: models, stories and the corpus, one test binary.
//! See `TESTING.md` at the repository root for the layout and the rules.

mod common;

mod model_args;
mod model_cmux;
mod model_demux;
mod model_framing;
mod model_reassembler;
mod model_sms;
mod model_urc;
mod model_ussd;

mod story_at;
mod story_cmux;
mod story_framing;
mod story_sms;
mod story_urc;
mod story_ussd;

mod corpus_at;
mod corpus_cmux;
mod corpus_framing;
mod corpus_gsm7;
mod corpus_sms;
mod corpus_urc;
mod corpus_ussd;
