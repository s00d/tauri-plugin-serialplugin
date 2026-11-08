//! Background polling for available-port list changes (desktop + shared diff helpers).

use crate::error::Error;
use crate::events::{PortListEvent, PortListSink, WatchPortsOptions};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, ThreadId};
use std::time::Duration;

const DEFAULT_POLL_MS: u64 = 2000;
const MIN_POLL_MS: u64 = 250;
const MAX_POLL_MS: u64 = 30_000;

struct Subscriber {
    channel: PortListSink,
    single_port_per_device: bool,
    poll_interval_ms: u64,
    last: HashMap<String, HashMap<String, String>>,
    /// USB location per path at the last poll (not part of the events).
    last_locations: LocationMap,
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
type LocationMap = HashMap<String, String>;
type Listing = (PortListMap, LocationMap);

/// Ports that appeared (`added`) and disappeared (`removed`), each sorted by path. A
/// path now holding another device (type, vid, pid or serial number changed) is in
/// both; strings filled in late (manufacturer, product, a value replacing "Unknown")
/// are the same device and in neither.
pub fn diff_ports(old: &PortListMap, new: &PortListMap) -> PortDiff {
    diff_listing(old, new, &HashMap::new(), &HashMap::new())
}

/// Device identity in the 4.x map; "Unknown" (not read yet) matches anything.
fn other_device(old: &PortInfoMap, new: &PortInfoMap) -> bool {
    const IDENTITY: [&str; 4] = ["type", "vid", "pid", "serial_number"];
    let known = |v: Option<&String>| v.filter(|v| v.as_str() != crate::state::UNKNOWN).cloned();
    IDENTITY
        .iter()
        .any(|k| matches!((known(old.get(*k)), known(new.get(*k))), (Some(a), Some(b)) if a != b))
}

/// [`diff_ports`], also treating a changed USB location on the same path as a re-plug
/// (identical devices without serial numbers). A location missing on either side is
/// not a change.
fn diff_listing(
    old: &PortListMap,
    new: &PortListMap,
    old_loc: &LocationMap,
    new_loc: &LocationMap,
) -> PortDiff {
    let replugged = |k: &String| match (old.get(k), new.get(k)) {
        (Some(o), Some(n)) => {
            other_device(o, n)
                || matches!((old_loc.get(k), new_loc.get(k)), (Some(a), Some(b)) if a != b)
        }
        _ => false,
    };
    let mut added: Vec<_> = new
        .iter()
        .filter(|(k, _)| !old.contains_key(*k) || replugged(k))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let mut removed: Vec<_> = old
        .keys()
        .filter(|k| !new.contains_key(*k) || replugged(k))
        .cloned()
        .collect();
    added.sort_by(|a, b| a.0.cmp(&b.0));
    removed.sort();
    (added, removed)
}

/// Returns false when the receiver is gone.
fn send_event(channel: &PortListSink, event: PortListEvent) -> bool {
    let alive = channel(event);
    if !alive {
        crate::log_warn!("Failed to send port list event: receiver is gone");
    }
    alive
}

/// Held while the poller sends a batch, so `unsubscribe` can wait for it.
static DELIVERY: Mutex<()> = Mutex::new(());

/// The poller thread while it sends a batch (it holds `DELIVERY`).
static DELIVERING: Mutex<Option<ThreadId>> = Mutex::new(None);

fn delivering_here() -> bool {
    *crate::sync_util::lock_or_recover(&DELIVERING) == Some(thread::current().id())
}

fn still_subscribed(id: u32, channel: &PortListSink) -> bool {
    crate::sync_util::lock_or_recover(monitor_state())
        .subscribers
        .get(&id)
        .is_some_and(|s| Arc::ptr_eq(&s.channel, channel))
}

#[cfg(test)]
static TEST_LISTING: Mutex<Option<Listing>> = Mutex::new(None);

/// Serializes the tests that use the global monitor.
#[cfg(test)]
static TEST_SERIAL: Mutex<()> = Mutex::new(());

/// Tests: enumerate this listing instead of the system's (`None` restores it).
#[cfg(test)]
fn set_test_listing(listing: Option<Listing>) {
    *crate::sync_util::lock_or_recover(&TEST_LISTING) = listing;
}

fn enumerate(single_port_per_device: bool) -> Result<Listing, Error> {
    #[cfg(test)]
    if let Some(l) = crate::sync_util::lock_or_recover(&TEST_LISTING).clone() {
        return Ok(l);
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        crate::port::list::enumerate_with_locations(single_port_per_device)
    }
    #[cfg(target_os = "ios")]
    {
        Ok((
            crate::port::list::enumerate_available_ports(single_port_per_device)?,
            HashMap::new(),
        ))
    }
    #[cfg(target_os = "android")]
    {
        match ANDROID_ENUM.get() {
            Some(f) => Ok((f(single_port_per_device)?, HashMap::new())),
            None => Err(Error::String(
                "port list monitor requires desktop or android build".into(),
            )),
        }
    }
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

/// Diff `current` against the subscriber's last view, store it, return the events
/// (removals first, so a re-plug on one path reads removed + added).
fn take_events(sub: &mut Subscriber, current: &Listing) -> Vec<PortListEvent> {
    let (added, removed) = diff_listing(&sub.last, &current.0, &sub.last_locations, &current.1);
    if !added.is_empty() || !removed.is_empty() {
        crate::log_info!(
            "[SerialEnumerate] poll delta +{} -{} total={}",
            added.len(),
            removed.len(),
            current.0.len()
        );
    }
    sub.last = current.0.clone();
    sub.last_locations = current.1.clone();
    removed
        .into_iter()
        .map(|path| PortListEvent::Removed { path })
        .chain(
            added
                .into_iter()
                .map(|(path, info)| PortListEvent::Added { path, info }),
        )
        .collect()
}

/// Enumerate and send outside the state lock: enumeration can take hundreds of ms,
/// and a sink may call back into the monitor (subscribe / unsubscribe).
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
    let batches: Vec<(u32, PortListSink, Vec<PortListEvent>)> = {
        let mut guard = crate::sync_util::lock_or_recover(monitor_state());
        guard
            .subscribers
            .iter_mut()
            .filter_map(|(id, sub)| {
                let events = take_events(sub, lists.get(&sub.single_port_per_device)?);
                (!events.is_empty()).then(|| (*id, sub.channel.clone(), events))
            })
            .collect()
    };
    let mut dead = Vec::new();
    {
        let _delivery = crate::sync_util::lock_or_recover(&DELIVERY);
        *crate::sync_util::lock_or_recover(&DELIVERING) = Some(thread::current().id());
        for (id, channel, events) in batches {
            let mut alive = true;
            for event in events {
                // Unsubscribed meanwhile (maybe by this very sink): stop here.
                if !still_subscribed(id, &channel) {
                    break;
                }
                alive &= send_event(&channel, event);
            }
            if !alive {
                dead.push((id, channel));
            }
        }
        *crate::sync_util::lock_or_recover(&DELIVERING) = None;
    }
    let mut guard = crate::sync_util::lock_or_recover(monitor_state());
    for (id, channel) in dead {
        // Only drop it if the id was not unsubscribed and reused meanwhile.
        if guard
            .subscribers
            .get(&id)
            .is_some_and(|s| Arc::ptr_eq(&s.channel, &channel))
        {
            guard.subscribers.remove(&id);
        }
    }
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

/// 4.0.0 API (feature `tauri`): [`subscribe_sink`] for a Tauri channel.
#[cfg(feature = "tauri")]
pub fn subscribe(
    channel_id: u32,
    channel: tauri::ipc::Channel<PortListEvent>,
    options: WatchPortsOptions,
) -> Result<(), Error> {
    subscribe_sink(channel_id, crate::events::channel_sink(channel), options)
}

/// Subscribe to available-port list changes. Sends an initial [`PortListEvent::Snapshot`].
pub fn subscribe_sink(
    channel_id: u32,
    channel: PortListSink,
    options: WatchPortsOptions,
) -> Result<(), Error> {
    if is_subscribed(channel_id) {
        return Err(Error::new(format!(
            "port list watch id {channel_id} is already in use"
        )));
    }
    let single = options.single_port_per_device.unwrap_or(false);
    let poll_ms = clamp_poll_ms(options.poll_interval_ms.unwrap_or(DEFAULT_POLL_MS));
    let (snapshot, locations) = enumerate(single).map_err(|e| {
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
            last_locations: locations,
        },
    );
    state.poll_interval_ms = min_poll_interval(&state.subscribers);
    ensure_thread(&mut state);
    Ok(())
}

pub fn is_subscribed(channel_id: u32) -> bool {
    crate::sync_util::lock_or_recover(monitor_state())
        .subscribers
        .contains_key(&channel_id)
}

/// Unsubscribe from port list changes. Once it returns the sink gets no more events
/// (it waits for a batch the poller is sending; called from inside a sink, it
/// returns at once and the rest of that batch is skipped).
pub fn unsubscribe(channel_id: u32) {
    {
        let mut state = crate::sync_util::lock_or_recover(monitor_state());
        state.subscribers.remove(&channel_id);
        state.poll_interval_ms = min_poll_interval(&state.subscribers);
        // Wake the thread so it notices the empty set and exits promptly.
        state.wake.store(true, Ordering::SeqCst);
    }
    if !delivering_here() {
        drop(crate::sync_util::lock_or_recover(&DELIVERY));
    }
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
    #[cfg(not(target_os = "android"))]
    #[test]
    fn unsubscribe_does_not_block_and_thread_exits() {
        let _serial = crate::sync_util::lock_or_recover(&TEST_SERIAL);
        let channel: PortListSink = Arc::new(|_| true);
        subscribe_sink(
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

    fn usb(vid: &str) -> PortInfoMap {
        HashMap::from([
            ("type".to_string(), "USB".to_string()),
            ("vid".to_string(), vid.to_string()),
        ])
    }

    #[test]
    fn diff_reports_replug_with_other_device_as_removed_and_added() {
        let old = HashMap::from([("COM1".to_string(), usb("1"))]);
        let new = HashMap::from([("COM1".to_string(), usb("2"))]);
        let (added, removed) = diff_ports(&old, &new);
        assert_eq!(added, vec![("COM1".to_string(), usb("2"))]);
        assert_eq!(removed, vec!["COM1".to_string()]);
        assert_eq!(diff_ports(&old, &old), (vec![], vec![]));
    }

    #[test]
    fn diff_reports_location_change_as_replug() {
        let ports = HashMap::from([("COM1".to_string(), usb("1"))]);
        let loc = |l: &str| HashMap::from([("COM1".to_string(), l.to_string())]);
        let (added, removed) = diff_listing(&ports, &ports, &loc("1-2"), &loc("1-3"));
        assert_eq!(added.len(), 1);
        assert_eq!(removed, vec!["COM1".to_string()]);
        let (added, removed) = diff_listing(&ports, &ports, &loc("1-2"), &loc("1-2"));
        assert!(added.is_empty() && removed.is_empty());
        // A location that could not be read is not a re-plug.
        let (added, removed) = diff_listing(&ports, &ports, &loc("1-2"), &HashMap::new());
        assert!(added.is_empty() && removed.is_empty());
    }

    fn usb_full(serial: &str, manufacturer: &str, product: &str) -> PortInfoMap {
        HashMap::from([
            ("type".to_string(), "USB".to_string()),
            ("vid".to_string(), "1234".to_string()),
            ("pid".to_string(), "5678".to_string()),
            ("serial_number".to_string(), serial.to_string()),
            ("manufacturer".to_string(), manufacturer.to_string()),
            ("product".to_string(), product.to_string()),
        ])
    }

    /// udev / IOKit fill manufacturer and product strings late: the same device, so
    /// no `removed` + `added` (4.x sent nothing). Another serial is another device.
    #[test]
    fn late_metadata_is_not_a_replug() {
        let one = |info: PortInfoMap| HashMap::from([("/dev/ttyUSB0".to_string(), info)]);
        let before = one(usb_full("A1", "Unknown", "Unknown"));
        let after = one(usb_full("A1", "FTDI", "FT232R"));
        assert_eq!(diff_ports(&before, &after), (vec![], vec![]));
        let filled = one(usb_full("Unknown", "Unknown", "Unknown"));
        assert_eq!(diff_ports(&filled, &before), (vec![], vec![]));
        let other = one(usb_full("B2", "FTDI", "FT232R"));
        let (added, removed) = diff_ports(&after, &other);
        assert_eq!((added.len(), removed.len()), (1, 1));
    }

    /// Once `unsubscribe` returns, the sink gets nothing more (also not the rest of a
    /// batch the poller was sending).
    #[test]
    fn no_events_after_unsubscribe_returns() {
        let _serial = crate::sync_util::lock_or_recover(&TEST_SERIAL);
        set_test_listing(Some((HashMap::new(), HashMap::new())));
        let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let tx = Mutex::new(tx);
        let seen = count.clone();
        let channel: PortListSink = Arc::new(move |event| {
            if let PortListEvent::Added { .. } = event {
                seen.fetch_add(1, Ordering::SeqCst);
                let _ = crate::sync_util::lock_or_recover(&tx).send(());
                thread::sleep(Duration::from_millis(200));
            }
            true
        });
        subscribe_sink(7_010, channel, opts()).unwrap();
        set_test_listing(Some((
            HashMap::from([
                ("/dev/fake1".to_string(), usb("1")),
                ("/dev/fake2".to_string(), usb("2")),
                ("/dev/fake3".to_string(), usb("3")),
            ]),
            HashMap::new(),
        )));
        request_refresh();
        rx.recv_timeout(Duration::from_secs(5))
            .expect("first added");
        unsubscribe(7_010);
        let after = count.load(Ordering::SeqCst);
        thread::sleep(Duration::from_millis(600));
        set_test_listing(None);
        assert_eq!(
            count.load(Ordering::SeqCst),
            after,
            "event after unsubscribe"
        );
    }

    fn wait_until(what: &str, f: impl Fn() -> bool) {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !f() {
            assert!(std::time::Instant::now() < deadline, "timed out: {what}");
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn opts() -> WatchPortsOptions {
        WatchPortsOptions {
            poll_interval_ms: Some(MIN_POLL_MS),
            single_port_per_device: None,
        }
    }

    /// A sink that subscribes / unsubscribes from inside its callback used to deadlock
    /// (events were sent under the monitor lock).
    #[test]
    fn callback_can_subscribe_and_unsubscribe() {
        let _serial = crate::sync_util::lock_or_recover(&TEST_SERIAL);
        set_test_listing(Some((HashMap::new(), HashMap::new())));
        let (tx, rx) = std::sync::mpsc::channel::<&'static str>();
        let tx = Mutex::new(tx);
        let channel: PortListSink = Arc::new(move |event| {
            if let PortListEvent::Added { .. } = event {
                assert!(is_subscribed(7_001));
                subscribe_sink(7_002, Arc::new(|_| true), opts()).unwrap();
                unsubscribe(7_002);
                unsubscribe(7_001);
                let _ = crate::sync_util::lock_or_recover(&tx).send("done");
            }
            true
        });
        subscribe_sink(7_001, channel, opts()).unwrap();
        set_test_listing(Some((
            HashMap::from([("/dev/fake0".to_string(), usb("1"))]),
            HashMap::new(),
        )));
        request_refresh();
        let got = rx.recv_timeout(Duration::from_secs(5));
        set_test_listing(None);
        assert_eq!(got, Ok("done"), "callback hung inside the monitor");
        wait_until("subscribers gone", || {
            !is_subscribed(7_001) && !is_subscribed(7_002)
        });
    }

    /// Same path, other device (vid or location changed) between two polls:
    /// `removed` then `added`, in the 4.x event format.
    #[test]
    fn replug_on_same_path_is_removed_then_added() {
        let _serial = crate::sync_util::lock_or_recover(&TEST_SERIAL);
        let path = "/dev/ttyUSB0".to_string();
        let listing = |vid: &str, loc: &str| {
            (
                HashMap::from([(path.clone(), usb(vid))]),
                HashMap::from([(path.clone(), loc.to_string())]),
            )
        };
        set_test_listing(Some(listing("1", "1-2")));
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink_events = events.clone();
        let channel: PortListSink = Arc::new(move |event| {
            crate::sync_util::lock_or_recover(&sink_events).push(event);
            true
        });
        subscribe_sink(7_003, channel, opts()).unwrap();
        let count = || crate::sync_util::lock_or_recover(&events).len();

        set_test_listing(Some(listing("2", "1-2")));
        request_refresh();
        wait_until("vid re-plug", || count() >= 3);
        set_test_listing(Some(listing("2", "1-4")));
        request_refresh();
        wait_until("location re-plug", || count() >= 5);
        unsubscribe(7_003);
        set_test_listing(None);

        let json: Vec<serde_json::Value> = crate::sync_util::lock_or_recover(&events)
            .iter()
            .map(|e| serde_json::to_value(e).unwrap())
            .collect();
        let added = serde_json::json!({"kind": "added", "path": path, "info": usb("2")});
        let removed = serde_json::json!({"kind": "removed", "path": path});
        assert_eq!(json[0]["kind"], "snapshot", "{json:?}");
        assert_eq!(json[1..], [removed.clone(), added.clone(), removed, added]);
    }
}
