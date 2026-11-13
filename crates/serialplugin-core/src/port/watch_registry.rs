//! Watch ids keyed by channel id: the 4.x global registry. Each
//! [`SerialManager`](crate::SerialManager) keeps its own registry (one old watch per
//! port and manager) and mirrors its watches here, so these functions still see them.

use crate::events::{EventSink, SerialEvent};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

static REGISTRY: OnceLock<Mutex<HashMap<u32, String>>> = OnceLock::new();

fn registry() -> &'static Mutex<HashMap<u32, String>> {
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn register(channel_id: u32, path: String) -> Result<(), crate::error::Error> {
    let mut map = crate::sync_util::lock_or_recover(registry());
    if map.contains_key(&channel_id) {
        return Err(crate::error::Error::new(format!(
            "watch id {} is already in use",
            channel_id
        )));
    }
    if map.values().any(|p| p == &path) {
        return Err(crate::error::Error::new(format!(
            "A watch is already active for port {}",
            path
        )));
    }
    map.insert(channel_id, path);
    Ok(())
}

pub fn contains(channel_id: u32) -> bool {
    crate::sync_util::lock_or_recover(registry()).contains_key(&channel_id)
}

pub fn unregister(channel_id: u32) -> Option<String> {
    crate::sync_util::lock_or_recover(registry()).remove(&channel_id)
}

/// Mirror of a manager's own watch (the manager registry is the authority; this
/// keeps [`contains`] / [`paths_for_port`] showing every manager's watches, as in 4.x).
/// `false` (nothing changed) when `channel_id` is already there, from any manager.
pub(crate) fn mirror_insert(channel_id: u32, path: &str) -> bool {
    let mut map = crate::sync_util::lock_or_recover(registry());
    if map.contains_key(&channel_id) {
        return false;
    }
    map.insert(channel_id, path.to_string());
    true
}

pub(crate) fn mirror_remove(channel_id: u32) {
    let mut map = crate::sync_util::lock_or_recover(registry());
    map.remove(&channel_id);
}

pub fn paths_for_port(path: &str) -> Vec<u32> {
    crate::sync_util::lock_or_recover(registry())
        .iter()
        .filter(|(_, p)| p.as_str() == path)
        .map(|(id, _)| *id)
        .collect()
}

/// 4.0.0 API (feature `tauri`): send one event on a Tauri channel.
#[cfg(feature = "tauri")]
pub fn send_event(channel: &tauri::ipc::Channel<SerialEvent>, event: SerialEvent) {
    if let Err(e) = channel.send(event) {
        crate::log_warn!("Failed to send serial event on channel: {}", e);
    }
}

pub fn send_event_to(channel: &EventSink, event: SerialEvent) {
    if channel(event) {
        return;
    }
    crate::log_warn!("Failed to send serial event: receiver is gone");
}

#[cfg(test)]
mod tests {
    #[test]
    fn duplicate_watch_id_is_rejected() {
        let id = 0xD0D0_0001;
        assert!(!super::contains(id));
        super::register(id, "dup-a".into()).unwrap();
        assert!(super::contains(id));
        assert!(super::register(id, "dup-b".into()).is_err());
        // One watch per port, whatever the id.
        assert!(super::register(id + 1, "dup-a".into()).is_err());
        super::register(id + 1, "dup-c".into()).unwrap();
        assert_eq!(super::unregister(id).as_deref(), Some("dup-a"));
        assert_eq!(super::unregister(id + 1).as_deref(), Some("dup-c"));
        assert!(!super::contains(id));
    }
}
