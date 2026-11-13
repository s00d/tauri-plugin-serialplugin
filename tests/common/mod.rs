//! Fixtures shared by the models and stories: the plugin behind real Tauri IPC (mock
//! runtime, the plugin's default permission set as the ACL), devices (PTY pair, TCP
//! listener), event channels into `std::sync::mpsc`, and polling helpers.
#![allow(dead_code, unused_imports)]

pub use proptest::prelude::*;
pub use proptest::test_runner::{Config, TestRunner};
pub use serde_json::{json, Value};
pub use std::io::{Read, Write};
pub use std::net::{TcpListener, TcpStream};
pub use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
pub use std::sync::mpsc::{self, Receiver};
pub use std::sync::{Arc, Mutex};
pub use std::time::{Duration, Instant};
pub use tauri::ipc::{Channel, InvokeResponseBody};
pub use tauri::test::MockRuntime;
pub use tauri::Manager;
pub use tauri_plugin_serialplugin::api::SerialPort;
pub use tauri_plugin_serialplugin::events::{SerialEvent, WatchOptions};
pub use tauri_plugin_serialplugin::state::PortState;

use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{get_ipc_response, mock_builder, mock_context, noop_assets, INVOKE_KEY};
use tauri::webview::InvokeRequest;
use tauri_utils::acl::resolved::{Resolved, ResolvedCommand, ResolvedCommandReference};
use tauri_utils::acl::ExecutionContext;

/// Upper bound for something that should happen at once (an event, a reply, a release).
pub const WAIT: Duration = Duration::from_secs(2);

pub fn repo_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Commands granted by `permissions/default.toml` (`allow-foo-bar` → `foo_bar`).
pub fn default_permission_commands() -> Vec<String> {
    let text = std::fs::read_to_string(repo_root().join("permissions/default.toml"))
        .expect("permissions/default.toml");
    text.lines()
        .filter_map(|l| l.trim().trim_end_matches(',').strip_prefix("\"allow-"))
        .map(|p| p.trim_end_matches('"').replace('-', "_"))
        .collect()
}

/// The resolved ACL of an app granting `commands` on window `main`.
fn acl_for(commands: &[String]) -> Resolved {
    let mut acl = Resolved::default();
    for cmd in commands {
        acl.allowed_commands.insert(
            format!("plugin:serialplugin|{cmd}"),
            vec![ResolvedCommand::new(
                ExecutionContext::Local,
                ResolvedCommandReference::new("default".into(), "serialplugin:default".into()),
                vec![glob::Pattern::new("main").unwrap()],
                Vec::new(),
                None,
            )],
        );
    }
    acl
}

/// Callback ids for IPC channels, unique in the process (as the JS side hands them out).
static NEXT_CALLBACK: AtomicU32 = AtomicU32::new(1000);

/// The plugin registered in a mock app, reached through IPC from webview `main`.
pub struct Plugin {
    pub app: tauri::App<MockRuntime>,
    pub webview: tauri::WebviewWindow<MockRuntime>,
}

impl Plugin {
    pub fn new() -> Self {
        Self::with_config(None).expect("plugin init")
    }

    /// With `plugins."serialplugin-config"` set to `config` in tauri.conf.
    pub fn with_config(config: Option<Value>) -> Result<Self, String> {
        Self::build(config, &default_permission_commands())
    }

    /// An app whose ACL grants only `commands` (with `serialplugin:` permissions).
    pub fn with_commands(commands: &[String]) -> Self {
        Self::build(None, commands).expect("plugin init")
    }

    fn build(config: Option<Value>, commands: &[String]) -> Result<Self, String> {
        let mut context = mock_context(noop_assets());
        *context.runtime_authority_mut() =
            tauri::runtime_authority!(Default::default(), acl_for(commands));
        if let Some(config) = config {
            context
                .config_mut()
                .plugins
                .0
                .insert("serialplugin-config".into(), config);
        }
        let app = mock_builder()
            .plugin(tauri_plugin_serialplugin::init())
            .build(context)
            .map_err(|e| e.to_string())?;
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("webview");
        Ok(Self { app, webview })
    }

    /// `invoke('plugin:serialplugin|<cmd>', args)` as guest-js sends it.
    pub fn invoke(&self, cmd: &str, args: Value) -> Result<Value, Value> {
        invoke(&self.webview, cmd, args)
    }

    /// The managed state, for what IPC cannot show (settings on the handle, hub buffers)
    /// and for Rust-side watches whose events land in an mpsc receiver.
    pub fn sp(&self) -> SerialPort<MockRuntime> {
        self.app.state::<SerialPort<MockRuntime>>().inner().clone()
    }

    pub fn managed(&self) -> Vec<String> {
        let mut v: Vec<String> = serde_json::from_value(
            self.invoke("managed_ports", json!({}))
                .expect("managed_ports"),
        )
        .unwrap();
        v.sort();
        v
    }
}

/// `invoke` from any thread (the webview handle is shared).
pub fn invoke(
    webview: &tauri::WebviewWindow<MockRuntime>,
    cmd: &str,
    args: Value,
) -> Result<Value, Value> {
    let request = InvokeRequest {
        cmd: format!("plugin:serialplugin|{cmd}"),
        callback: CallbackFn(0),
        error: CallbackFn(1),
        url: if cfg!(windows) {
            "http://tauri.localhost"
        } else {
            "tauri://localhost"
        }
        .parse()
        .unwrap(),
        body: InvokeBody::Json(args),
        headers: Default::default(),
        invoke_key: INVOKE_KEY.to_string(),
    };
    get_ipc_response(webview, request)
        .map(|body| body.deserialize::<Value>().expect("json response"))
}

/// A JS-side channel argument (`__CHANNEL__:<id>`) and its id. Its events go to the
/// webview, which the mock runtime does not keep: use [`event_channel`] to see events.
pub fn ipc_channel() -> (u32, Value) {
    let id = NEXT_CALLBACK.fetch_add(1, Ordering::Relaxed);
    (id, json!(format!("__CHANNEL__:{id}")))
}

/// A Rust-side channel whose events arrive in an mpsc receiver.
pub fn event_channel() -> (Channel<SerialEvent>, Receiver<SerialEvent>) {
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    let channel = Channel::<SerialEvent>::new(move |body| {
        if let InvokeResponseBody::Json(json) = body {
            let _ = tx
                .lock()
                .unwrap()
                .send(serde_json::from_str(&json).expect("SerialEvent json"));
        }
        Ok(())
    });
    (channel, rx)
}

/// Next `Data` payload on `rx` (other events skipped), within `WAIT`.
pub fn next_data(rx: &Receiver<SerialEvent>) -> Vec<u8> {
    let deadline = Instant::now() + WAIT;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left).expect("data event") {
            SerialEvent::Data { data, .. } => return data,
            _ => continue,
        }
    }
}

/// Poll `done` until it holds; fail with `what` after `WAIT`.
pub fn eventually(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !done() {
        assert!(Instant::now() < deadline, "{what}");
        std::thread::yield_now();
    }
}

/// Bytes the RX hub of `path` holds for `read` (`usize::MAX` when not connected).
pub fn hub_buffered(sp: &SerialPort<MockRuntime>, path: &str) -> usize {
    let map = sp.serialports.lock().unwrap();
    match map.get(path).map(|i| &i.state) {
        Some(PortState::Connected(cp)) => cp
            .rx_hub
            .lock()
            .unwrap()
            .as_ref()
            .map_or(0, |h| h.shared().buffered_len()),
        _ => usize::MAX,
    }
}

/// Settings on the plugin's read handle of `path`.
pub struct HandleSettings {
    pub baud: u32,
    pub data_bits: serialport::DataBits,
    pub flow: serialport::FlowControl,
    pub parity: serialport::Parity,
    pub stop: serialport::StopBits,
}

pub fn handle_settings(sp: &SerialPort<MockRuntime>, path: &str) -> HandleSettings {
    let map = sp.serialports.lock().unwrap();
    let Some(PortState::Connected(cp)) = map.get(path).map(|i| &i.state) else {
        panic!("{path} is not connected");
    };
    let handle = cp.port.clone();
    drop(map);
    let p = handle.lock().unwrap();
    HandleSettings {
        baud: p.baud_rate().unwrap(),
        data_bits: p.data_bits().unwrap(),
        flow: p.flow_control().unwrap(),
        parity: p.parity().unwrap(),
        stop: p.stop_bits().unwrap(),
    }
}

/// Device-side I/O: a PTY master or the TCP peer of the plugin's connection.
pub trait Io: Read + Write + Send {}
impl<X: Read + Write + Send> Io for X {}

/// Read from the device side until `done` holds or `WAIT` passes.
pub fn recv_until(io: &mut dyn Io, done: impl Fn(&[u8]) -> bool) -> Vec<u8> {
    let deadline = Instant::now() + WAIT;
    let mut got = Vec::new();
    let mut buf = [0u8; 256];
    while !done(&got) && Instant::now() < deadline {
        match io.read(&mut buf) {
            Ok(n) => got.extend_from_slice(&buf[..n]),
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::TimedOut
                        | std::io::ErrorKind::WouldBlock
                        | std::io::ErrorKind::Interrupted
                ) => {}
            // Linux: EIO on a master while no slave is open; wait for it.
            Err(_) => std::thread::yield_now(),
        }
    }
    got
}

/// Device thread: read one command (up to `\r`), answer `reply`, return the command.
pub fn responder(mut io: Box<dyn Io>, reply: &'static [u8]) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let cmd = recv_until(io.as_mut(), |b| b.contains(&b'\r'));
        let _ = io.write_all(reply);
        let _ = io.flush();
        cmd
    })
}

/// A PTY pair: the master is the device, the slave goes into the plugin by
/// `insert_port` (macOS cannot open a PTY slave by path: `IOSSIOSPEED` → ENOTTY).
#[cfg(unix)]
pub fn pty() -> (serialport::TTYPort, serialport::TTYPort, String) {
    // The lock only guards `pair()` (ptsname is not thread-safe on macOS).
    let (guard, mut master, slave) = tauri_plugin_serialplugin::sync_util::pty_pair_locked();
    drop(guard);
    let path = serialport::SerialPort::name(&slave).expect("slave name");
    serialport::SerialPort::set_timeout(&mut master, Duration::from_millis(20)).unwrap();
    (master, slave, path)
}

/// A PTY managed by the plugin under its slave path; returns the master and the path.
#[cfg(unix)]
pub fn pty_port(plugin: &Plugin) -> (serialport::TTYPort, String) {
    let (master, slave, path) = pty();
    plugin
        .sp()
        .insert_port(path.clone(), Box::new(slave))
        .expect("insert pty");
    (master, path)
}

/// A TCP bridge: listener plus the `tcp://` path the plugin opens.
pub fn tcp_listener() -> (TcpListener, String) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let path = format!("tcp://{}", listener.local_addr().unwrap());
    (listener, path)
}

/// The connection the plugin's `open` made (accepted without blocking forever).
pub fn accept(listener: &TcpListener) -> TcpStream {
    listener.set_nonblocking(true).unwrap();
    let deadline = Instant::now() + WAIT;
    let stream = loop {
        match listener.accept() {
            Ok((s, _)) => break s,
            Err(_) if Instant::now() < deadline => std::thread::yield_now(),
            Err(e) => panic!("plugin did not connect: {e}"),
        }
    };
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_millis(20)))
        .unwrap();
    stream
}

/// Open `path` over IPC with guest-js's argument shape.
pub fn open_ipc(plugin: &Plugin, path: &str) -> Result<Value, Value> {
    plugin.invoke(
        "open",
        json!({ "path": path, "baudRate": 115200, "timeout": 200 }),
    )
}

/// Each key of `input` comes back with the same value after a round trip through the
/// Rust type guest-js's JSON lands in: a renamed serde key would be dropped silently.
pub fn keys_survive<T: serde::de::DeserializeOwned + serde::Serialize>(input: Value) {
    let back = serde_json::to_value(serde_json::from_value::<T>(input.clone()).unwrap()).unwrap();
    for (key, value) in input.as_object().unwrap() {
        assert_eq!(&back[key], value, "{key} of {}", std::any::type_name::<T>());
    }
}
