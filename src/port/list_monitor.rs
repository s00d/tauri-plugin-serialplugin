//! Background polling for available-port list changes (desktop + shared diff helpers).

use crate::error::Error;
use crate::events::{PortListEvent, WatchPortsOptions};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
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
    stop: Arc<AtomicBool>,
    wake: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl MonitorState {
    fn new() -> Self {
        Self {
            subscribers: HashMap::new(),
            poll_interval_ms: DEFAULT_POLL_MS,
            stop: Arc::new(AtomicBool::new(false)),
            wake: Arc::new(AtomicBool::new(false)),
            thread: None,
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

fn send_event(channel: &Channel<PortListEvent>, event: PortListEvent) {
    if let Err(e) = channel.send(event) {
        crate::log_warn!("Failed to send port list event: {}", e);
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
static ANDROID_ENUM: OnceLock<
    Box<dyn Fn(bool) -> Result<HashMap<String, HashMap<String, String>>, Error> + Send + Sync>,
> = OnceLock::new();

#[cfg(target_os = "android")]
pub fn set_android_enumerator<F>(f: F)
where
    F: Fn(bool) -> Result<HashMap<String, HashMap<String, String>>, Error> + Send + Sync + 'static,
{
    let _ = ANDROID_ENUM.set(Box::new(f));
}

fn poll_subscriber(sub: &mut Subscriber) {
    let Ok(current) = enumerate(sub.single_port_per_device) else {
        return;
    };
    let (added, removed) = diff_ports(&sub.last, &current);
    for (path, info) in added {
        send_event(&sub.channel, PortListEvent::Added { path, info });
    }
    for path in removed {
        send_event(&sub.channel, PortListEvent::Removed { path });
    }
    sub.last = current;
}

fn poll_all(subscribers: &mut HashMap<u32, Subscriber>) {
    for sub in subscribers.values_mut() {
        poll_subscriber(sub);
    }
}

fn ensure_thread(state: &mut MonitorState) {
    if state.thread.is_some() || state.subscribers.is_empty() {
        return;
    }
    state.stop.store(false, Ordering::SeqCst);
    state.wake.store(false, Ordering::SeqCst);
    let stop = state.stop.clone();
    let wake = state.wake.clone();
    state.thread = Some(thread::spawn(move || loop {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        let interval = {
            let guard = monitor_state().lock().unwrap();
            clamp_poll_ms(guard.poll_interval_ms)
        };
        let mut slept = 0u64;
        while slept < interval && !stop.load(Ordering::SeqCst) {
            if wake.swap(false, Ordering::SeqCst) {
                break;
            }
            thread::sleep(Duration::from_millis(50));
            slept += 50;
        }
        if stop.load(Ordering::SeqCst) {
            break;
        }
        if let Ok(mut guard) = monitor_state().lock() {
            poll_all(&mut guard.subscribers);
        }
    }));
}

fn stop_thread_if_idle(state: &mut MonitorState) {
    if !state.subscribers.is_empty() {
        return;
    }
    state.stop.store(true, Ordering::SeqCst);
    state.wake.store(true, Ordering::SeqCst);
    if let Some(handle) = state.thread.take() {
        let _ = handle.join();
    }
}

/// Subscribe to available-port list changes. Sends an initial [`PortListEvent::Snapshot`].
pub fn subscribe(
    channel_id: u32,
    channel: Channel<PortListEvent>,
    options: WatchPortsOptions,
) -> Result<(), Error> {
    let single = options.single_port_per_device.unwrap_or(false);
    let poll_ms = clamp_poll_ms(options.poll_interval_ms.unwrap_or(DEFAULT_POLL_MS));
    let snapshot = enumerate(single)?;
    send_event(
        &channel,
        PortListEvent::Snapshot {
            ports: snapshot.clone(),
        },
    );

    let mut state = monitor_state()
        .lock()
        .map_err(|_| Error::new("port list monitor lock poisoned"))?;

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
    let Ok(mut state) = monitor_state().lock() else {
        return;
    };
    state.subscribers.remove(&channel_id);
    stop_thread_if_idle(&mut state);
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
}
