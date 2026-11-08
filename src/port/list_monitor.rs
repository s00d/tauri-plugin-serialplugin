//! Background polling for available-port list changes (desktop + shared diff helpers).

use crate::error::Error;
use crate::events::{PortListEvent, WatchPortsOptions};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::Duration;
use tauri::ipc::Channel;

const DEFAULT_POLL_MS: u64 = 2000;
const MIN_POLL_MS: u64 = 250;
const MAX_POLL_MS: u64 = 30_000;

struct Subscriber {
    channel: Channel<PortListEvent>,
    single_port_per_device: bool,
    poll_interval_ms: u64,
    last: HashMap<String, HashMap<String, String>>,
}

struct MonitorState {
    subscribers: HashMap<u32, Subscriber>,
    poll_interval_ms: u64,
    wake: Arc<AtomicBool>,
    /// Poll thread is alive (cleared by the thread itself under the lock).
    running: bool,
}

impl MonitorState {
    fn new() -> Self {
        Self {
            subscribers: HashMap::new(),
            poll_interval_ms: DEFAULT_POLL_MS,
            wake: Arc::new(AtomicBool::new(false)),
            running: false,
        }
    }
}

fn monitor_state() -> &'static Mutex<MonitorState> {
    static STATE: OnceLock<Mutex<MonitorState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(MonitorState::new()))
}

fn clamp_poll_ms(ms: u64) -> u64 {
    ms.clamp(MIN_POLL_MS, MAX_POLL_MS)
}

fn min_poll_interval(subscribers: &HashMap<u32, Subscriber>) -> u64 {
    subscribers
        .values()
        .map(|s| s.poll_interval_ms)
        .min()
        .unwrap_or(DEFAULT_POLL_MS)
}

type PortInfoMap = HashMap<String, String>;
type PortListMap = HashMap<String, PortInfoMap>;
type PortDiff = (Vec<(String, PortInfoMap)>, Vec<String>);

pub fn diff_ports(old: &PortListMap, new: &PortListMap) -> PortDiff {
    let old_keys: HashSet<&str> = old.keys().map(String::as_str).collect();
    let new_keys: HashSet<&str> = new.keys().map(String::as_str).collect();

    let added = new
        .iter()
        .filter(|(k, _)| !old_keys.contains(k.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    let removed = old
        .keys()
        .filter(|k| !new_keys.contains(k.as_str()))
        .cloned()
        .collect();

    (added, removed)
}

/// Returns false when the webview side of the channel is gone.
fn send_event(channel: &Channel<PortListEvent>, event: PortListEvent) -> bool {
    match channel.send(event) {
        Ok(()) => true,
        Err(e) => {
            crate::log_warn!("Failed to send port list event: {}", e);
            false
        }
    }
}

#[cfg(desktop)]
fn enumerate(
    single_port_per_device: bool,
) -> Result<HashMap<String, HashMap<String, String>>, Error> {
    crate::port::list::enumerate_available_ports(single_port_per_device)
}

#[cfg(not(desktop))]
fn enumerate(
    single_port_per_device: bool,
) -> Result<HashMap<String, HashMap<String, String>>, Error> {
    let _ = single_port_per_device;
    #[cfg(target_os = "android")]
    {
        if let Some(f) = ANDROID_ENUM.get() {
            return f(single_port_per_device);
        }
    }
    Err(Error::String(
        "port list monitor requires desktop or android build".into(),
    ))
}

#[cfg(target_os = "android")]
type AndroidEnumerator =
    Box<dyn Fn(bool) -> Result<HashMap<String, HashMap<String, String>>, Error> + Send + Sync>;

#[cfg(target_os = "android")]
static ANDROID_ENUM: OnceLock<AndroidEnumerator> = OnceLock::new();

#[cfg(target_os = "android")]
pub fn set_android_enumerator<F>(f: F)
where
    F: Fn(bool) -> Result<HashMap<String, HashMap<String, String>>, Error> + Send + Sync + 'static,
{
    let _ = ANDROID_ENUM.set(Box::new(f));
}

/// Diff `current` against the subscriber's last view and send events.
/// Returns false when the subscriber's channel is dead and it should be dropped.
fn deliver(sub: &mut Subscriber, current: &PortListMap) -> bool {
    let (added, removed) = diff_ports(&sub.last, current);
    if !added.is_empty() || !removed.is_empty() {
        crate::log_info!(
            "[SerialEnumerate] poll delta +{} -{} total={}",
            added.len(),
            removed.len(),
            current.len()
        );
    }
    let mut alive = true;
    for (path, info) in added {
        alive &= send_event(&sub.channel, PortListEvent::Added { path, info });
    }
    for path in removed {
        alive &= send_event(&sub.channel, PortListEvent::Removed { path });
    }
    sub.last = current.clone();
    alive
}

/// Enumerate outside the state lock (it can take hundreds of ms), then deliver under it.
fn poll_once() {
    let modes: HashSet<bool> = {
        let guard = crate::sync_util::lock_or_recover(monitor_state());
        guard
            .subscribers
            .values()
            .map(|s| s.single_port_per_device)
            .collect()
    };
    let mut lists = HashMap::new();
    for single in modes {
        match enumerate(single) {
            Ok(v) => {
                lists.insert(single, v);
            }
            Err(e) => crate::log_warn!("[SerialEnumerate] poll failed: {}", e),
        }
    }
    let mut guard = crate::sync_util::lock_or_recover(monitor_state());
    guard
        .subscribers
        .retain(|_, sub| match lists.get(&sub.single_port_per_device) {
            Some(current) => deliver(sub, current),
            None => true,
        });
    guard.poll_interval_ms = min_poll_interval(&guard.subscribers);
}

/// Start the poll thread if needed. The thread exits on its own (under the state
/// lock) once no subscribers remain, so nobody ever has to join it while holding
/// the lock it needs.
fn ensure_thread(state: &mut MonitorState) {
    if state.running || state.subscribers.is_empty() {
        return;
    }
    state.running = true;
    let wake = state.wake.clone();
    thread::spawn(move || loop {
        let interval = {
            let mut guard = crate::sync_util::lock_or_recover(monitor_state());
            if guard.subscribers.is_empty() {
                guard.running = false;
                return;
            }
            clamp_poll_ms(guard.poll_interval_ms)
        };
        let mut slept = 0u64;
        while slept < interval {
            if wake.swap(false, Ordering::SeqCst) {
                break;
            }
            thread::sleep(Duration::from_millis(50));
            slept += 50;
        }
        poll_once();
    });
}

/// Subscribe to available-port list changes. Sends an initial [`PortListEvent::Snapshot`].
pub fn subscribe(
    channel_id: u32,
    channel: Channel<PortListEvent>,
    options: WatchPortsOptions,
) -> Result<(), Error> {
    let single = options.single_port_per_device.unwrap_or(false);
    let poll_ms = clamp_poll_ms(options.poll_interval_ms.unwrap_or(DEFAULT_POLL_MS));
    let snapshot = enumerate(single).map_err(|e| {
        crate::log_error!("[SerialEnumerate] watch snapshot failed: {}", e);
        e
    })?;
    crate::log_info!(
        "[SerialEnumerate] watch snapshot {} port(s) single={}",
        snapshot.len(),
        single
    );
    if !send_event(
        &channel,
        PortListEvent::Snapshot {
            ports: snapshot.clone(),
        },
    ) {
        return Err(Error::new("port list channel is closed"));
    }

    let mut state = crate::sync_util::lock_or_recover(monitor_state());

    state.subscribers.insert(
        channel_id,
        Subscriber {
            channel,
            single_port_per_device: single,
            poll_interval_ms: poll_ms,
            last: snapshot,
        },
    );
    state.poll_interval_ms = min_poll_interval(&state.subscribers);
    ensure_thread(&mut state);
    Ok(())
}

/// Unsubscribe from port list changes.
pub fn unsubscribe(channel_id: u32) {
    let mut state = crate::sync_util::lock_or_recover(monitor_state());
    state.subscribers.remove(&channel_id);
    state.poll_interval_ms = min_poll_interval(&state.subscribers);
    // Wake the thread so it notices the empty set and exits promptly.
    state.wake.store(true, Ordering::SeqCst);
}

/// Request an immediate poll (e.g. after platform hotplug hint).
pub fn request_refresh() {
    if let Ok(state) = monitor_state().lock() {
        if !state.subscribers.is_empty() {
            state.wake.store(true, Ordering::SeqCst);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_detects_added_and_removed() {
        let old = HashMap::from([
            (
                "COM1".to_string(),
                HashMap::from([("type".to_string(), "USB".to_string())]),
            ),
            (
                "COM2".to_string(),
                HashMap::from([("type".to_string(), "USB".to_string())]),
            ),
        ]);
        let new = HashMap::from([
            (
                "COM2".to_string(),
                HashMap::from([("type".to_string(), "USB".to_string())]),
            ),
            (
                "COM3".to_string(),
                HashMap::from([("type".to_string(), "USB".to_string())]),
            ),
        ]);
        let (added, removed) = diff_ports(&old, &new);
        assert_eq!(added.len(), 1);
        assert_eq!(added[0].0, "COM3");
        assert_eq!(removed, vec!["COM1".to_string()]);
    }

    /// Regression: unsubscribe used to join the poll thread while holding the lock the
    /// thread needed, freezing the caller. Now it returns at once and the thread exits.
    #[cfg(desktop)]
    #[test]
    fn unsubscribe_does_not_block_and_thread_exits() {
        let channel: Channel<PortListEvent> = Channel::new(|_| Ok(()));
        subscribe(
            9_999,
            channel,
            WatchPortsOptions {
                poll_interval_ms: Some(MIN_POLL_MS),
                single_port_per_device: None,
            },
        )
        .unwrap();
        thread::sleep(Duration::from_millis(MIN_POLL_MS + 100));
        let t = std::time::Instant::now();
        unsubscribe(9_999);
        assert!(t.elapsed() < Duration::from_millis(100));
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while crate::sync_util::lock_or_recover(monitor_state()).running {
            assert!(
                std::time::Instant::now() < deadline,
                "poll thread did not exit"
            );
            thread::sleep(Duration::from_millis(20));
        }
    }
}
