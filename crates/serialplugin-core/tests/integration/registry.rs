//! Stories: the per-manager watch registry and the shared 4.x id registry.

use crate::common::*;

/// Given two managers with the same path,
/// when watches are added,
/// then each manager has its own watch; a second old watch on a port and a taken id are refused, watch_core is unlimited.
#[test]
fn registry_is_per_manager_and_one_old_watch_per_port() {
    let (f1, f2) = (Fake::new(), Fake::new());
    let m1 = manager_with("same-path", f1.port(true));
    let m2 = manager_with("same-path", f2.port(true));
    m1.insert_port("other".into(), Fake::new().port(true))
        .unwrap();
    let (s, _r1) = old_sink();
    let id = m1
        .watch("same-path".into(), WatchOptions::default(), s)
        .unwrap();
    let (s, _r2) = old_sink();
    m2.watch("same-path".into(), WatchOptions::default(), s)
        .expect("other manager, same path");
    let (s, _r3) = old_sink();
    let err = m1
        .watch("same-path".into(), WatchOptions::default(), s)
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "A watch is already active for port same-path"
    );
    let (s, _r4) = old_sink();
    let err = m1
        .watch_with_id(id, "other".into(), WatchOptions::default(), s)
        .unwrap_err();
    assert_eq!(err.to_string(), format!("watch id {id} is already in use"));
    for _ in 0..3 {
        let (s, _r) = core_sink();
        m1.watch_core("same-path".into(), WatchOptions::default(), None, s)
            .expect("watch_core is unlimited");
    }
    m1.close_all().unwrap();
    m2.close_all().unwrap();
}

/// Given a watch under an id in one manager,
/// when another manager asks for the same id,
/// then it is refused and the global 4.x registry keeps the first.
#[test]
fn a_watch_id_taken_by_another_manager_is_refused() {
    use serialplugin_core::port::watch_registry as wr;
    let a = manager_with("id-a", Fake::new().port(true));
    let b = manager_with("id-b", Fake::new().port(true));
    let id = 0xABCD_0101;
    let (s, _ra) = old_sink();
    a.watch_with_id(id, "id-a".into(), WatchOptions::default(), s)
        .unwrap();
    let (s, _rb) = old_sink();
    let err = b
        .watch_with_id(id, "id-b".into(), WatchOptions::default(), s)
        .unwrap_err();
    assert_eq!(ErrorKind::from_message(&err.to_string()), ErrorKind::Busy);
    assert_eq!(wr::paths_for_port("id-a"), vec![id]);
    a.unwatch(id).unwrap();
    assert!(!wr::contains(id));
    a.close("id-a".into()).unwrap();
    b.close("id-b".into()).unwrap();
}

/// Given another manager holding the ids after a freed one,
/// when a new watch takes an id,
/// then it skips every id the other manager holds.
#[test]
fn watch_ids_skip_ids_held_by_another_manager() {
    let a = manager_with("ida", Fake::new().port(true));
    let (s, _rx) = old_sink();
    let n = a.watch("ida".into(), WatchOptions::default(), s).unwrap();
    a.unwatch(n).unwrap();
    let b = SerialManager::new();
    for k in 1..=30u32 {
        let p = format!("idb{k}");
        b.insert_port(p.clone(), Fake::new().port(false)).unwrap();
        let (s, _rx) = old_sink();
        // Taken by a test running in parallel: just as good.
        let _ = b.watch_with_id(n + k, p, WatchOptions::default(), s);
    }
    let (s, _rx) = old_sink();
    let id = a
        .watch("ida".into(), WatchOptions::default(), s)
        .expect("watch with a free id");
    assert!(
        !(n + 1..=n + 30).contains(&id),
        "{id} is held by the other manager"
    );
    b.close_all().unwrap();
    a.close("ida".into()).unwrap();
}
