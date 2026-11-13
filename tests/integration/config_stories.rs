//! Stories on plugin configuration (`plugins."serialplugin-config"` in tauri.conf).
//! Real hardware: `tests/modem_live.rs`.

use crate::common::*;

fn port_of(path: &str) -> u16 {
    path.rsplit(':').next().unwrap().parse().unwrap()
}

/// Given no plugin config
/// When the webview opens a TCP bridge
/// Then it connects (every address is allowed, as in 4.x)
#[test]
fn without_an_allowlist_every_tcp_address_opens() {
    for config in [None, Some(json!({})), Some(json!({ "tcpAllowlist": null }))] {
        let plugin = Plugin::with_config(config).expect("init");
        let (listener, path) = tcp_listener();
        assert_eq!(open_ipc(&plugin, &path), Ok(json!(path)));
        accept(&listener);
    }
}

/// Given `tcpAllowlist: ["LocalHost:<a>"]`
/// When the webview opens `tcp://localhost:<a>` and `tcp://127.0.0.1:<b>`
/// Then the listed address opens (host case-insensitive) and the other is refused
/// before anything connects
#[test]
fn the_allowlist_admits_listed_bridges_only() {
    let (listed, listed_path) = tcp_listener();
    let (other, other_path) = tcp_listener();
    let a = port_of(&listed_path);
    let plugin = Plugin::with_config(Some(json!({ "tcpAllowlist": [format!("LocalHost:{a}")] })))
        .expect("init");
    let path = format!("tcp://localhost:{a}");
    assert_eq!(open_ipc(&plugin, &path), Ok(json!(path)));
    accept(&listed);

    assert!(open_ipc(&plugin, &other_path).is_err());
    other.set_nonblocking(true).unwrap();
    assert!(other.accept().is_err(), "refused address connected");
    assert_eq!(plugin.managed(), vec![path]);
}

/// Given an empty allowlist
/// When the webview opens a TCP bridge
/// Then it is refused
#[test]
fn an_empty_allowlist_refuses_every_bridge() {
    let plugin = Plugin::with_config(Some(json!({ "tcpAllowlist": [] }))).expect("init");
    let (_listener, path) = tcp_listener();
    assert!(open_ipc(&plugin, &path).is_err());
    assert!(plugin.managed().is_empty());
}

/// Given an allowlist that is not a list of strings
/// When the app starts
/// Then plugin setup fails (never "allow everything")
#[test]
fn a_malformed_allowlist_fails_setup() {
    for bad in [json!("10.0.0.5:4000"), json!([1]), json!({ "a": 1 })] {
        assert!(Plugin::with_config(Some(json!({ "tcpAllowlist": bad }))).is_err());
    }
}
