//! serialplugin-core behaviour tests: models and stories, one test binary.
//! See `TESTING.md` at the repository root for the layout and the rules.
#![cfg(not(any(target_os = "android", target_os = "ios")))]

mod common;

mod model_config;
mod model_delivery;
mod model_exchange;
mod model_manager;
mod model_mux;
mod model_ports;
mod model_tcp;

mod story_at_session;
mod story_disconnect;
mod story_errors;
mod story_headless;
mod story_hub;
mod story_mux;
mod story_port_life;
mod story_read_write;
mod story_watch_delivery;
