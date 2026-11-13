//! All integration tests in one binary (links once instead of once per file).
//!
//! Models (random action sequences against a reference model) carry most of
//! the coverage; topic files hold properties and Given/When/Then stories for
//! what random steps cannot reach. Golden Java fixtures pin real device bytes.

#![cfg(feature = "fake-transport")]

mod common;

#[cfg(feature = "serialport-compat")]
mod adapter_model;
mod driver_model;
mod reader_model;

mod baud_divisors;
mod cdc_notifications;
mod fake_transport;
mod golden_parity;
mod helpers;
mod port_lifecycle;
mod probe;
mod rx_filters;
mod spike_fd;
