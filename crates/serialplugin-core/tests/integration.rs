//! All integration tests in one binary (one link and one macOS scan instead of one per
//! file). `common` holds the fixtures; `*_model` files are model tests over the public
//! API; the other files are stories (Given / When / Then) for what the models cannot
//! reach: timing, special configuration, transport failures.

mod common;

mod config_props;
mod manager_model;
mod tcp_model;

mod cmux;
mod delivery;
mod disconnect;
mod errors;
mod exchange;
mod headless;
mod port_handles;
mod registry;
