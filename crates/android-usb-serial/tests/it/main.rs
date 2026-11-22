//! android-usb-serial behaviour tests: models, stories and the corpus, one test binary over
//! `FakeTransport`. See `TESTING.md` at the repository root for the layout and the rules.

#![cfg(feature = "fake-transport")]

mod common;

#[cfg(feature = "serialport-compat")]
mod model_adapter;
mod model_baud;
mod model_driver;
mod model_probe;
mod model_reader;
mod model_transport;

mod story_cdc;
mod story_fake_modem;
mod story_hardware;
mod story_port;
mod story_probe;

mod corpus_golden;
