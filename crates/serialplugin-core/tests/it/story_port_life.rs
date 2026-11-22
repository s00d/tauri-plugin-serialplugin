//! Stories: one port from open to close and again, and the watch registries of several
//! managers.

use crate::common::*;
use serialplugin_core::error::ErrorKind;
use serialplugin_core::events::{CoreEvent, PortStatus, SerialEvent, WatchOptions};
use serialplugin_core::port::watch_registry as global;
use serialplugin_core::SerialManager;
use std::io;

/// Given a modem on a port,
/// when an app opens it, watches it, talks AT, loses it, opens it again and closes it,
/// then subscribers see each state in order, the watch ends with its Disconnect, the new
/// session starts clean, a closed port is unknown, and no handle is left open.
#[test]
fn open_use_lose_reopen_close() {
    let device = FakeDevice::with_modem(Modem::new());
    let m = SerialManager::new();
    let (states, states_rx) = sink::<CoreEvent>();
    m.subscribe_core(states);
    m.insert_port("life".into(), Box::new(device.port()))
        .unwrap();
    let (events, rx) = sink::<SerialEvent>();
    let watch = m.watch("life".into(), watch_opts(0, true), events).unwrap();
    device.send(b"+CREG: 1,1\r\nhello\r\n");
    assert_eq!(next_urc(&rx), "+CREG: 1,1");
    assert_eq!(next_data(&rx), b"hello\r\n");
    let reply = m.at("life".into(), "AT+CSQ".into(), None).unwrap();
    assert_eq!(reply.solicited_body, ["+CSQ: 20,99"]);
    device.fail_reads(io::ErrorKind::BrokenPipe);
    assert_eq!(next_disconnect(&rx).0, "life");
    device.wait_closed();
    let e = m.at_core("life".into(), "AT".into(), None).unwrap_err();
    assert_eq!(e.kind(), ErrorKind::Disconnected);
    m.unwatch(watch).expect("the watch is still registered");

    let again = FakeDevice::with_modem(Modem::new());
    m.insert_port("life".into(), Box::new(again.port()))
        .unwrap();
    let (events, rx) = sink::<SerialEvent>();
    m.watch("life".into(), watch_opts(0, false), events)
        .unwrap();
    again.send(b"fresh");
    assert_eq!(collect(&rx, 5), b"fresh");
    m.close("life".into()).unwrap();
    again.wait_closed();
    assert!(rest(rx).is_empty(), "events after close");
    let e = m.close_core("life".into()).unwrap_err();
    assert_eq!(e.kind(), ErrorKind::NotFound);
    assert!(m.managed_ports().unwrap().is_empty());
    let seen: Vec<_> = (0..4).map(|_| next_state(&states_rx, "life")).collect();
    assert_eq!(seen[0], PortStatus::Opened);
    assert!(matches!(seen[1], PortStatus::Disconnected { .. }));
    assert_eq!(seen[2..], [PortStatus::Opened, PortStatus::Closed]);
}

/// Given two managers with the same path,
/// when watches are added under free, taken and given ids, and through the 4.x global
/// registry,
/// then each manager has its own watch per port, an id is unique across managers and is
/// never handed out twice, `watch_core` is unlimited, and the registry shows every watch.
#[test]
fn watch_ids_are_unique_across_managers() {
    let port = || FakeDevice::new().port();
    let (a, b) = (manager_with("same", port()), manager_with("same", port()));
    a.insert_port("other".into(), Box::new(port())).unwrap();
    let watch =
        |m: &SerialManager, path: &str| m.watch(path.into(), WatchOptions::default(), sink().0);
    let with_id = |m: &SerialManager, id, path: &str| {
        m.watch_with_id(id, path.into(), WatchOptions::default(), sink().0)
    };
    let id = watch(&a, "same").unwrap();
    let theirs = watch(&b, "same").expect("another manager, the same path");
    assert_ne!(id, theirs);
    assert_eq!(
        watch(&a, "same").unwrap_err().to_string(),
        "A watch is already active for port same"
    );
    let taken = with_id(&a, id, "other").unwrap_err();
    assert_eq!(
        taken.to_string(),
        format!("watch id {id} is already in use")
    );
    for _ in 0..3 {
        a.watch_core("same".into(), WatchOptions::default(), None, sink().0)
            .expect("watch_core is unlimited");
    }
    b.insert_port("theirs".into(), Box::new(port())).unwrap();
    let taken = with_id(&b, id, "theirs").unwrap_err();
    assert_eq!(kind(taken), ErrorKind::Busy);
    let mut shown = global::paths_for_port("same");
    shown.sort();
    let mut want = vec![id, theirs];
    want.sort();
    assert_eq!(shown, want);

    // Ids another manager holds are skipped: hold the next ones.
    a.unwatch(id).unwrap();
    assert!(!global::contains(id));
    for k in 1..=10 {
        let path = format!("next{k}");
        b.insert_port(path.clone(), Box::new(port())).unwrap();
        // Taken by a test running in parallel: just as good.
        let _ = with_id(&b, theirs + k, &path);
    }
    let fresh = watch(&a, "same").unwrap();
    assert!(!(theirs + 1..=theirs + 10).contains(&fresh));

    // The 4.x functions themselves.
    let own = 0xD0D0_0001;
    global::register(own, "own-a".into()).unwrap();
    assert!(global::contains(own));
    assert!(global::register(own, "own-b".into()).is_err());
    assert!(global::register(own + 1, "own-a".into()).is_err());
    assert_eq!(global::unregister(own).as_deref(), Some("own-a"));
    assert!(!global::contains(own));
    a.close_all().unwrap();
    b.close_all().unwrap();
    assert!(!global::contains(fresh), "a closed port keeps no watch id");
}
