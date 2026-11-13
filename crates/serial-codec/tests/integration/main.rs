//! All integration tests in one binary (one link and one first-launch scan per
//! `cargo test`): shared helpers, models, topic stories/properties and spec vectors.

mod common;

mod demux_model;
mod framing_model;
mod reassembler_model;

mod args;
mod at;
mod cmux;
mod framing;
mod sms;
mod urc;

mod at_vectors;
mod cmux_vectors;
mod framing_vectors;
mod gsm7_vectors;
mod sms_vectors;
mod urc_vectors;
mod ussd_vectors;
