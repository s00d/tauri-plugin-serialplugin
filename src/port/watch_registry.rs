//! Tracks active desktop watch sessions keyed by channel id.

use crate::events::SerialEvent;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use tauri::ipc::Channel;

static REGISTRY: OnceLock<Mutex<HashMap<u32, String>>> = OnceLock::new();

fn registry() -> &'static Mutex<HashMap<u32, String>> {
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn register(channel_id: u32, path: String) -> Result<(), crate::error::Error> {
    let mut map = crate::sync_util::lock_or_recover(registry());
    if map.values().any(|p| p == &path) {
        return Err(crate::error::Error::new(format!(
            "A watch is already active for port {}",
            path
        )));
    }
    map.insert(channel_id, path);
    Ok(())
}

pub fn unregister(channel_id: u32) -> Option<String> {
    crate::sync_util::lock_or_recover(registry()).remove(&channel_id)
}

pub fn paths_for_port(path: &str) -> Vec<u32> {
    crate::sync_util::lock_or_recover(registry())
        .iter()
        .filter(|(_, p)| p.as_str() == path)
        .map(|(id, _)| *id)
        .collect()
}

pub fn send_event(channel: &Channel<SerialEvent>, event: SerialEvent) {
    if let Err(e) = channel.send(event) {
        crate::log_warn!("Failed to send serial event on channel: {}", e);
    }
}
