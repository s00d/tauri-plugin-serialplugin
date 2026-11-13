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
    if channel(event) {
        return true;
    }
    crate::log_warn!("Failed to send port list event: receiver is gone");
    false
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
    sub.last = current.0.clone();
    sub.last_locations = current.1.clone();
    let counts = (added.len(), removed.len());
    let events: Vec<PortListEvent> = removed
        .into_iter()
        .map(|path| PortListEvent::Removed { path })
        .chain(
            added
                .into_iter()
                .map(|(path, info)| PortListEvent::Added { path, info }),
        )
        .collect();
    if events.is_empty() {
        return events;
    }
    crate::log_info!(
        "[SerialEnumerate] poll delta +{} -{} total={}",
        counts.0,
        counts.1,
        current.0.len()
    );
    events
}

/// One poll at a time (the poll thread, or a test driving the monitor by hand), so a
/// change is diffed and sent by exactly one of them.
static POLL: Mutex<()> = Mutex::new(());

/// Enumerate and send outside the state lock: enumeration can take hundreds of ms,
/// and a sink may call back into the monitor (subscribe / unsubscribe).
fn poll_once() {
    let _poll = crate::sync_util::lock_or_recover(&POLL);
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
    // Called right after a subscriber was added (under the same lock), so the set
    // is never empty here.
    if state.running {
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
        // 50 ms slices until `interval` (rounded up) or a wake-up.
        for _ in (0..interval).step_by(50) {
            if wake.swap(false, Ordering::SeqCst) {
                break;
            }
            thread::sleep(Duration::from_millis(50));
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

    fn usb(vid: &str) -> PortInfoMap {
        HashMap::from([
            ("type".to_string(), "USB".to_string()),
            ("vid".to_string(), vid.to_string()),
        ])
    }

    /// Upper bound for something that happens at once (a hand-off between threads).
    const PROMPT: Duration = Duration::from_secs(1);

    fn wait_until(what: &str, f: impl Fn() -> bool) {
        let deadline = std::time::Instant::now() + 2 * PROMPT;
        while !f() {
            assert!(std::time::Instant::now() < deadline, "timed out: {what}");
            thread::sleep(Duration::from_millis(5));
        }
    }

    /// Slow polls: in these tests only `poll_once` and `request_refresh` poll.
    fn opts() -> WatchPortsOptions {
        WatchPortsOptions {
            poll_interval_ms: Some(MAX_POLL_MS),
            single_port_per_device: None,
        }
    }

    fn running() -> bool {
        crate::sync_util::lock_or_recover(monitor_state()).running
    }

    /// No subscribers, no poll thread, an empty test listing.
    fn reset_monitor() {
        set_test_listing(Some((HashMap::new(), HashMap::new())));
        let ids: Vec<u32> = crate::sync_util::lock_or_recover(monitor_state())
            .subscribers
            .keys()
            .copied()
            .collect();
        for id in ids {
            unsubscribe(id);
        }
        wait_until("poll thread exits", || !running());
    }

    fn listing(ports: &[(&str, &str)]) -> Listing {
        (
            ports
                .iter()
                .map(|(path, vid)| (path.to_string(), usb(vid)))
                .collect(),
            HashMap::new(),
        )
    }

    fn recording_sink() -> (PortListSink, Arc<Mutex<Vec<PortListEvent>>>) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink_events = events.clone();
        let sink: PortListSink = Arc::new(move |event| {
            crate::sync_util::lock_or_recover(&sink_events).push(event);
            true
        });
        (sink, events)
    }

    fn json(events: &Mutex<Vec<PortListEvent>>) -> Vec<serde_json::Value> {
        crate::sync_util::lock_or_recover(events)
            .iter()
            .map(|e| serde_json::to_value(e).unwrap())
            .collect()
    }

    fn subscriber(channel: PortListSink, poll_interval_ms: u64) -> Subscriber {
        Subscriber {
            channel,
            single_port_per_device: false,
            poll_interval_ms,
            last: HashMap::new(),
            last_locations: HashMap::new(),
        }
    }

    /// The bookkeeping helpers, driven directly (no poll thread).
    #[test]
    fn monitor_bookkeeping() {
        let _serial = crate::sync_util::lock_or_recover(&TEST_SERIAL);
        reset_monitor();
        let a: PortListSink = Arc::new(|_| true);
        let b: PortListSink = Arc::new(|_| true);

        let mut subs = HashMap::new();
        assert_eq!(min_poll_interval(&subs), DEFAULT_POLL_MS);
        subs.insert(1, subscriber(a.clone(), 900));
        subs.insert(2, subscriber(a.clone(), 400));
        assert_eq!(min_poll_interval(&subs), 400);

        {
            let mut state = crate::sync_util::lock_or_recover(monitor_state());
            state.subscribers.insert(7_030, subscriber(a.clone(), 700));
            state.subscribers.insert(7_031, subscriber(b.clone(), 300));
            state.poll_interval_ms = min_poll_interval(&state.subscribers);
            state.wake.store(false, Ordering::SeqCst);
        }
        assert!(is_subscribed(7_030) && !is_subscribed(7_032));
        assert!(still_subscribed(7_030, &a));
        assert!(!still_subscribed(7_030, &b), "another sink under that id");
        assert!(!still_subscribed(7_032, &a));
        let wake = crate::sync_util::lock_or_recover(monitor_state())
            .wake
            .clone();
        request_refresh();
        assert!(
            wake.swap(false, Ordering::SeqCst),
            "refresh wakes the poller"
        );

        // `unsubscribe` waits for a batch another thread is sending.
        let delivery = crate::sync_util::lock_or_recover(&DELIVERY);
        let (tx, rx) = std::sync::mpsc::channel();
        thread::spawn(move || {
            unsubscribe(7_031);
            tx.send(()).unwrap();
        });
        assert!(rx.recv_timeout(Duration::from_millis(50)).is_err());
        drop(delivery);
        rx.recv_timeout(PROMPT)
            .expect("unsubscribe returns after the batch");
        assert!(!is_subscribed(7_031));
        assert!(
            wake.swap(false, Ordering::SeqCst),
            "unsubscribe wakes the poller"
        );
        assert_eq!(
            crate::sync_util::lock_or_recover(monitor_state()).poll_interval_ms,
            700
        );
        unsubscribe(7_030);
        assert!(!is_subscribed(7_030));
        wake.store(false, Ordering::SeqCst);
        request_refresh();
        assert!(!wake.load(Ordering::SeqCst), "nobody to poll for");

        assert!(!delivering_here());
        *crate::sync_util::lock_or_recover(&DELIVERING) = Some(thread::current().id());
        assert!(delivering_here());
        let other = thread::spawn(|| thread::current().id()).join().unwrap();
        *crate::sync_util::lock_or_recover(&DELIVERING) = Some(other);
        assert!(!delivering_here());
        *crate::sync_util::lock_or_recover(&DELIVERING) = None;
    }

    /// The monitor end to end, in one test because it is global. The step that has a
    /// sink call back into the monitor comes last.
    #[test]
    fn monitor_sequence() {
        let _serial = crate::sync_util::lock_or_recover(&TEST_SERIAL);
        reset_monitor();

        // `SerialManager::watch_ports*` hand out the id they registered, and
        // `unwatch_ports` removes it.
        let m = crate::SerialManager::new();
        let sink: PortListSink = Arc::new(|_| true);
        assert_eq!(
            m.watch_ports_with_id(7_020, opts(), sink.clone()).unwrap(),
            7_020
        );
        assert!(is_subscribed(7_020));
        assert!(running(), "subscribing starts the poll thread");
        assert!(m.watch_ports_with_id(7_020, opts(), sink.clone()).is_err());
        let a = m.watch_ports(opts(), sink.clone()).unwrap();
        let b = m.watch_ports(opts(), sink).unwrap();
        assert_ne!(a, b);
        assert!(is_subscribed(a) && is_subscribed(b));
        for id in [7_020, a, b] {
            m.unwatch_ports(id).unwrap();
            assert!(!is_subscribed(id));
        }

        // Same path, other device (vid or location changed) between two polls:
        // `removed` then `added`, in the 4.x event format.
        let path = "/dev/ttyUSB0";
        let located = |vid: &str, loc: &str| {
            (
                HashMap::from([(path.to_string(), usb(vid))]),
                HashMap::from([(path.to_string(), loc.to_string())]),
            )
        };
        set_test_listing(Some(located("1", "1-2")));
        let (sink, events) = recording_sink();
        subscribe_sink(7_003, sink, opts()).unwrap();
        set_test_listing(Some(located("2", "1-2")));
        poll_once();
        set_test_listing(Some(located("2", "1-4")));
        poll_once();
        poll_once();
        unsubscribe(7_003);
        let json = json(&events);
        let added = serde_json::json!({"kind": "added", "path": path, "info": usb("2")});
        let removed = serde_json::json!({"kind": "removed", "path": path});
        assert_eq!(json[0]["kind"], "snapshot", "{json:?}");
        assert_eq!(json[0]["ports"][path], serde_json::json!(usb("1")));
        assert_eq!(json[1..], [removed.clone(), added.clone(), removed, added]);

        // A sink whose receiver is gone is dropped at the next event.
        set_test_listing(Some(listing(&[])));
        let gone: PortListSink = Arc::new(|e| matches!(e, PortListEvent::Snapshot { .. }));
        subscribe_sink(7_004, gone, opts()).unwrap();
        set_test_listing(Some(listing(&[("/dev/fake0", "1")])));
        poll_once();
        assert!(!is_subscribed(7_004), "dead sink still subscribed");

        // `request_refresh` makes the poll thread poll now (not after 30 s).
        let (sink, events) = recording_sink();
        subscribe_sink(7_005, sink, opts()).unwrap();
        set_test_listing(Some(listing(&[("/dev/fake0", "1"), ("/dev/fake1", "1")])));
        request_refresh();
        wait_until("refresh poll", || {
            crate::sync_util::lock_or_recover(&events).len() >= 2
        });
        unsubscribe(7_005);

        // Once `unsubscribe` returns, the sink gets nothing more (also not the rest
        // of a batch the poller is sending).
        let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let tx = Mutex::new(tx);
        let seen = count.clone();
        let slow: PortListSink = Arc::new(move |event| {
            if let PortListEvent::Added { .. } = event {
                seen.fetch_add(1, Ordering::SeqCst);
                let _ = crate::sync_util::lock_or_recover(&tx).send(());
                thread::sleep(Duration::from_millis(50));
            }
            true
        });
        set_test_listing(Some(listing(&[])));
        subscribe_sink(7_010, slow, opts()).unwrap();
        set_test_listing(Some(listing(&[
            ("/dev/fake1", "1"),
            ("/dev/fake2", "2"),
            ("/dev/fake3", "3"),
        ])));
        let poller = thread::spawn(poll_once);
        rx.recv_timeout(PROMPT).expect("first added");
        unsubscribe(7_010);
        poller.join().unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 1, "event after unsubscribe");

        // With nobody left the poll thread ends by itself.
        wait_until("poll thread exits", || !running());

        // A sink may subscribe / unsubscribe from inside its callback (events were
        // once sent under the monitor lock, which deadlocked).
        set_test_listing(Some(listing(&[])));
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
        set_test_listing(Some(listing(&[("/dev/fake0", "1")])));
        thread::spawn(poll_once);
        let got = rx.recv_timeout(PROMPT);
        set_test_listing(None);
        assert_eq!(got, Ok("done"), "callback hung inside the monitor");
        assert!(!is_subscribed(7_001) && !is_subscribed(7_002));
    }

    // ------------------------------------------------ properties and the monitor model
    // In `src/`: the model injects the port list (`set_test_listing`) and polls by hand
    // (`poll_once`), and the diff is a private function.

    use proptest::prelude::*;
    use proptest::test_runner::{Config, TestRunner};

    const PATHS: [&str; 4] = ["/dev/a", "/dev/b", "/dev/c", "/dev/d"];

    /// Port info with identity keys that may be "Unknown" (not read yet) and a
    /// manufacturer string that is no part of the identity.
    fn info() -> impl Strategy<Value = PortInfoMap> {
        let v = || prop::sample::select(vec![crate::state::UNKNOWN, "1", "2"]);
        (v(), v(), v(), v()).prop_map(|(kind, vid, serial, maker)| {
            HashMap::from([
                ("type".to_string(), kind.to_string()),
                ("vid".to_string(), vid.to_string()),
                ("serial_number".to_string(), serial.to_string()),
                ("manufacturer".to_string(), maker.to_string()),
            ])
        })
    }

    fn path() -> impl Strategy<Value = String> {
        prop::sample::select(PATHS.to_vec()).prop_map(String::from)
    }

    fn ports() -> impl Strategy<Value = PortListMap> {
        prop::collection::hash_map(path(), info(), 0..4)
    }

    fn locations() -> impl Strategy<Value = LocationMap> {
        let loc = prop::sample::select(vec!["1-1", "1-2"]).prop_map(String::from);
        prop::collection::hash_map(path(), loc, 0..4)
    }

    /// Another device on the same path: an identity key known on both sides differs,
    /// or both USB locations are known and differ.
    fn replugged(
        o: &PortInfoMap,
        n: &PortInfoMap,
        ol: Option<&String>,
        nl: Option<&String>,
    ) -> bool {
        let known =
            |m: &PortInfoMap, k: &str| m.get(k).filter(|v| *v != crate::state::UNKNOWN).cloned();
        ["type", "vid", "pid", "serial_number"]
            .iter()
            .any(|k| matches!((known(o, k), known(n, k)), (Some(a), Some(b)) if a != b))
            || matches!((ol, nl), (Some(a), Some(b)) if a != b)
    }

    fn strictly_sorted<'a>(mut keys: impl Iterator<Item = &'a String>) -> bool {
        let Some(mut prev) = keys.next() else {
            return true;
        };
        keys.all(|k| std::mem::replace(&mut prev, k) < k)
    }

    proptest! {
        #![proptest_config(Config { cases: 256, failure_persistence: None, ..Config::default() })]

        /// Applying the diff to the old list gives the new one (a path that kept its
        /// device keeps its old info); unchanged and same-device entries get no event.
        #[test]
        fn diff_applied_to_old_gives_new(
            old in ports(), new in ports(), old_loc in locations(), new_loc in locations()
        ) {
            let (added, removed) = diff_listing(&old, &new, &old_loc, &new_loc);
            prop_assert!(strictly_sorted(added.iter().map(|(k, _)| k)));
            prop_assert!(strictly_sorted(removed.iter()));
            let mut applied = old.clone();
            for path in &removed {
                prop_assert!(applied.remove(path).is_some(), "removed {} was not listed", path);
            }
            for (path, info) in &added {
                prop_assert_eq!(Some(info), new.get(path));
                prop_assert!(applied.insert(path.clone(), info.clone()).is_none());
            }
            let mut keys: Vec<_> = applied.keys().collect();
            let mut want: Vec<_> = new.keys().collect();
            keys.sort();
            want.sort();
            prop_assert_eq!(keys, want);
            for (path, n) in &new {
                let Some(o) = old.get(path) else { continue };
                let replug = replugged(o, n, old_loc.get(path), new_loc.get(path));
                prop_assert_eq!(removed.contains(path), replug, "{}", path);
                if !replug {
                    prop_assert_eq!(&applied[path], o, "same device keeps its info");
                }
            }
            prop_assert_eq!(diff_ports(&old, &new), diff_listing(&old, &new, &HashMap::new(), &HashMap::new()));
        }

        #[test]
        fn poll_interval_is_clamped(ms in any::<u64>()) {
            let c = clamp_poll_ms(ms);
            prop_assert!((MIN_POLL_MS..=MAX_POLL_MS).contains(&c));
            prop_assert!(c == ms || !(MIN_POLL_MS..=MAX_POLL_MS).contains(&ms));
        }
    }

    /// Subscriber ids of the model (other tests use 7_0xx).
    const MODEL_IDS: [u32; 3] = [7_101, 7_102, 7_103];

    #[derive(Clone, Debug)]
    pub(super) enum MonitorStep {
        /// The system's port list changes (the injected enumerator).
        List(PortListMap, LocationMap),
        Poll,
        Subscribe(usize),
        Unsubscribe(usize),
    }

    #[derive(Clone, Debug, Default)]
    pub(super) struct MonitorModel {
        list: PortListMap,
        locations: LocationMap,
        subscribed: [bool; 3],
    }

    fn monitor_step() -> impl Strategy<Value = MonitorStep> {
        prop_oneof![
                3 => (ports(), locations()).prop_map(|(p, l)| MonitorStep::List(p, l)),
                3 => Just(MonitorStep::Poll),
                1 => (0usize..3).prop_map(MonitorStep::Subscribe),
                1 => (0usize..3).prop_map(MonitorStep::Unsubscribe),
        ]
    }

    impl MonitorModel {
        /// Subscribe a free slot, unsubscribe a used one (else the runner skips it).
        fn observable(&self, step: &MonitorStep) -> bool {
            let m = self;
            match step {
                MonitorStep::Subscribe(i) => !m.subscribed[*i],
                MonitorStep::Unsubscribe(i) => m.subscribed[*i],
                _ => true,
            }
        }

        fn apply(&mut self, step: &MonitorStep) {
            let m = self;
            match step {
                MonitorStep::List(p, l) => (m.list, m.locations) = (p.clone(), l.clone()),
                MonitorStep::Poll => {}
                MonitorStep::Subscribe(i) => m.subscribed[*i] = true,
                MonitorStep::Unsubscribe(i) => m.subscribed[*i] = false,
            }
        }
    }

    pub(super) struct MonitorSut {
        _serial: std::sync::MutexGuard<'static, ()>,
        events: [Option<Arc<Mutex<Vec<PortListEvent>>>>; 3],
        /// Whether the last step brought every subscriber up to date.
        fresh: bool,
    }

    /// The list a subscriber rebuilds from its snapshot and events.
    fn rebuild(events: &[PortListEvent]) -> PortListMap {
        let mut list = PortListMap::new();
        for e in events {
            match e {
                PortListEvent::Snapshot { ports } => list = ports.clone(),
                PortListEvent::Removed { path } => {
                    assert!(list.remove(path).is_some(), "removed {path} was not listed");
                }
                PortListEvent::Added { path, info } => {
                    assert!(
                        list.insert(path.clone(), info.clone()).is_none(),
                        "added {path} twice"
                    );
                }
            }
        }
        list
    }

    impl MonitorSut {
        fn new() -> MonitorSut {
            let serial = crate::sync_util::lock_or_recover(&TEST_SERIAL);
            // No wait for the poll thread to end (`reset_monitor`): it only polls when woken.
            set_test_listing(Some((HashMap::new(), HashMap::new())));
            MonitorSut {
                _serial: serial,
                events: Default::default(),
                fresh: false,
            }
        }

        fn apply(&mut self, m: &MonitorModel, step: MonitorStep) {
            let sut = self;
            sut.fresh = false;
            match step {
                MonitorStep::List(p, l) => set_test_listing(Some((p, l))),
                MonitorStep::Poll => {
                    poll_once();
                    sut.fresh = true;
                }
                MonitorStep::Subscribe(i) => {
                    let (sink, events) = recording_sink();
                    subscribe_sink(MODEL_IDS[i], sink, opts()).unwrap();
                    sut.events[i] = Some(events);
                }
                MonitorStep::Unsubscribe(i) => {
                    unsubscribe(MODEL_IDS[i]);
                    let events = sut.events[i].take().unwrap();
                    // Nothing more once `unsubscribe` returned (the woken poll thread
                    // may still poll for the others).
                    let n = events.lock().unwrap().len();
                    poll_once();
                    assert_eq!(events.lock().unwrap().len(), n, "event after unsubscribe");
                }
            }
            for (i, on) in m.subscribed.iter().enumerate() {
                assert_eq!(is_subscribed(MODEL_IDS[i]), *on);
            }
        }

        fn check(&self, m: &MonitorModel) {
            let sut = self;
            for events in sut.events.iter().flatten() {
                let events = events.lock().unwrap();
                assert!(matches!(
                    events.first(),
                    Some(PortListEvent::Snapshot { .. })
                ));
                let list = rebuild(&events);
                if !sut.fresh {
                    continue;
                }
                // Up to date after a poll: the same paths, each with the same device
                // (late metadata is not sent, a re-plug is).
                let mut keys: Vec<_> = list.keys().collect();
                let mut want: Vec<_> = m.list.keys().collect();
                keys.sort();
                want.sort();
                assert_eq!(keys, want, "{events:?}");
                for (path, info) in &m.list {
                    assert!(
                        !replugged(&list[path], info, None, None),
                        "{path}: {events:?}"
                    );
                }
            }
        }

        fn teardown(self) {
            let sut = self;
            for id in MODEL_IDS {
                unsubscribe(id);
            }
            set_test_listing(None);
            drop(sut);
        }
    }

    /// Random port lists, polls and (un)subscriptions: every subscriber rebuilds the
    /// list from its snapshot and events.
    #[test]
    fn subscribers_rebuild_the_port_list() {
        let mut runner = TestRunner::new(Config {
            cases: 32,
            failure_persistence: None,
            ..Config::default()
        });
        let result = runner.run(&prop::collection::vec(monitor_step(), 1..20), |steps| {
            let mut model = MonitorModel::default();
            let mut sut = MonitorSut::new();
            for step in steps {
                if !model.observable(&step) {
                    continue;
                }
                model.apply(&step);
                sut.apply(&model, step);
                sut.check(&model);
            }
            sut.teardown();
            Ok(())
        });
        if let Err(e) = result {
            panic!("{e}");
        }
    }
}
