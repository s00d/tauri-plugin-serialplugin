//! A real AT modem pulled out and put back by a person, three times. Only queries.
//!
//! `SERIALPLUGIN_REPLUG=/dev/cu.… cargo test --test it story_replug -- --nocapture`
//! prints UNPLUG / PLUG when it is time (cargo test: nextest ends a test that waits for a person).
//! Without `SERIALPLUGIN_REPLUG` the test prints SKIP and passes.

use crate::common::*;
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use tauri_plugin_serialplugin::error::ErrorKind;

/// How long a person may take to pull or put the plug.
const PERSON: Duration = Duration::from_secs(300);
/// How long a modem may take from power to the first `OK`.
const BOOT: Duration = Duration::from_secs(60);

struct Bench {
    plugin: Plugin,
    path: String,
    baud: u32,
    ports: Events,
    start: Instant,
}

impl Bench {
    fn say(&self, text: &str) {
        eprintln!("[{:7.2}] {text}", self.start.elapsed().as_secs_f32());
    }

    fn open(&self) -> Result<Value, Value> {
        let args = json!({ "path": self.path, "baudRate": self.baud, "timeout": 200 });
        self.plugin.invoke("open", args)
    }

    fn at(&self, path: &str, command: &str, timeout_ms: u64) -> Result<Value, Value> {
        let options = json!({ "timeoutMs": timeout_ms });
        let args = json!({ "path": path, "command": command, "options": options });
        self.plugin.invoke("at", args)
    }

    fn answers(&self, path: &str, within: Duration) -> bool {
        let deadline = Instant::now() + within;
        while Instant::now() < deadline {
            match self.at(path, "AT", 1000) {
                Ok(r) if r["status"] == "ok" => return true,
                Ok(_) => {}
                // The modem is still booting.
                Err(e) => assert_eq!(kind(Err(e)), ErrorKind::Timeout),
            }
        }
        false
    }

    /// The next list event about the modem's port.
    fn listed(&mut self, kind: &str) {
        let deadline = Instant::now() + PERSON;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let v = self.ports.message_within(left);
            let v = v.unwrap_or_else(|| panic!("nobody did it: no `{kind}`"));
            if v["path"] == self.path.as_str() {
                assert_eq!(v["kind"], kind, "{v}");
                return self.say(&format!("watch_ports: {v}"));
            }
        }
    }

    fn unplugged(&mut self) {
        self.listed("removed");
        let write = json!({ "path": self.path, "value": "AT\r" });
        assert_eq!(
            kind(self.plugin.invoke("write", write)),
            ErrorKind::Disconnected
        );
        assert_eq!(
            kind(self.at(&self.path, "AT", 1000)),
            ErrorKind::Disconnected
        );
        assert!(self.plugin.managed().contains(&self.path), "until close");
        self.say("write and at: Disconnected, path still managed");
    }

    fn plugged(&mut self) {
        self.say(">>> PLUG");
        self.listed("added");
        let opened = self.open();
        self.say(&format!("open: {opened:?}"));
        opened.expect("open after replug");
    }
}

#[test]
fn a_real_modem_survives_the_plug() {
    let Ok(path) = std::env::var("SERIALPLUGIN_REPLUG") else {
        eprintln!("SKIP a_real_modem_survives_the_plug: SERIALPLUGIN_REPLUG is not set");
        return;
    };
    let baud: u32 = std::env::var("SERIALPLUGIN_MODEM_BAUD")
        .ok()
        .and_then(|b| b.parse().ok())
        .unwrap_or(115_200);
    let plugin = Plugin::new();
    let mut ports = plugin.channel();
    let args = json!({ "options": { "pollIntervalMs": 250 }, "channel": ports.arg() });
    plugin.ok("watch_ports", args);
    assert_eq!(ports.message()["kind"], "snapshot");
    let mut bench = Bench {
        plugin,
        path,
        baud,
        ports,
        start: Instant::now(),
    };
    bench.open().expect("open");
    assert!(bench.answers(&bench.path, BOOT), "the modem answers");

    idle_watch_hears_the_unplug(&mut bench);
    a_running_exchange_ends(&mut bench);
    cmux_channels_go_with_the_port(&mut bench);
    bench.plugin.ok("close", json!({ "path": bench.path }));
    assert!(bench.plugin.managed().is_empty());
    bench.say("DONE");
}

/// Given a watch on the idle open port
/// When the modem is pulled out
/// Then the watch gets one disconnect, the port leaves the list, calls fail as disconnected
/// When it is put back, and the app closes the path and opens it
/// Then the modem answers, a new watch gets its bytes, and the old one nothing more
fn idle_watch_hears_the_unplug(b: &mut Bench) {
    let path = b.path.clone();
    let mut events = b.plugin.watch(&path, json!({}));
    b.say("=== 1/3 idle port with a watch");
    b.say(">>> UNPLUG");
    b.unplugged();
    loop {
        match events.next(&path) {
            Event::Disconnect => break b.say("watch: disconnect"),
            other => b.say(&format!("watch: {other:?}")),
        }
    }
    b.plugin.ok("close", json!({ "path": path }));
    assert!(b.plugin.managed().is_empty());
    b.plugged();
    assert!(b.answers(&path, BOOT), "the modem answers after replug");
    let mut again = b.plugin.watch(&path, json!({}));
    b.plugin
        .ok("write", json!({ "path": path, "value": "AT\r" }));
    let (end, mut got) = (Instant::now() + serialplugin_testing::WAIT, Vec::new());
    while !got.windows(4).any(|w: &[u8]| w == b"OK\r\n") {
        assert!(Instant::now() < end, "{:?}", String::from_utf8_lossy(&got));
        // A booting modem also sends URCs.
        if let Event::Data(data) = again.next(&path) {
            got.extend(data);
        }
    }
    assert_eq!(events.try_next(&path), None);
    b.plugin.unwatch(&again);
    b.say("1/3 ok");
}

/// Given exchanges running one after another and no watch
/// When the modem is pulled out
/// Then the running one ends as disconnected, not by its timeout
/// When it is put back and the app opens the path without closing it first
/// Then the modem answers
fn a_running_exchange_ends(b: &mut Bench) {
    let path = b.path.clone();
    b.say("=== 2/3 exchanges running");
    b.say(">>> UNPLUG");
    let deadline = Instant::now() + PERSON;
    let mut done = 0;
    let (error, took) = loop {
        assert!(Instant::now() < deadline, "nobody did it");
        let begin = Instant::now();
        match b.at(&path, "AT+CSQ", 30_000) {
            Ok(r) => {
                assert_eq!(r["timedOut"], false, "{r}");
                done += 1;
            }
            Err(e) => break (e, begin.elapsed()),
        }
    };
    b.say(&format!("exchange {done} ended after {took:?}: {error}"));
    assert_eq!(kind(Err(error)), ErrorKind::Disconnected);
    b.unplugged();
    b.plugged();
    assert!(b.answers(&path, BOOT), "the modem answers after replug");
    b.say("2/3 ok");
}

/// Given CMUX with two channels, one watched
/// When the modem is pulled out
/// Then the watch gets a disconnect and calls on both channels fail as disconnected
/// When it is put back and the app opens the port
/// Then the modem answers, if need be after CLD (it may have kept its power, and CMUX with it),
/// and CMUX starts again
fn cmux_channels_go_with_the_port(b: &mut Bench) {
    let path = b.path.clone();
    let enable = |b: &Bench| -> Vec<String> {
        b.plugin
            .invoke("enable_mux", json!({ "path": path }))
            .expect("enable_mux");
        let open = |dlci: u8| {
            let args = json!({ "path": path, "dlci": dlci });
            let channel = b.plugin.invoke("open_mux_channel", args).expect("channel");
            channel.as_str().unwrap().to_string()
        };
        vec![open(1), open(2)]
    };
    let channels = enable(b);
    for ch in &channels {
        assert!(b.answers(ch, BOOT), "{ch}");
    }
    let mut events = b.plugin.watch(&channels[0], json!({}));
    b.say("=== 3/3 CMUX with two channels");
    b.say(">>> UNPLUG");
    b.unplugged();
    loop {
        match events.next(&channels[0]) {
            Event::Disconnect => break b.say("channel watch: disconnect"),
            other => b.say(&format!("channel watch: {other:?}")),
        }
    }
    for ch in &channels {
        assert_eq!(kind(b.at(ch, "AT", 1000)), ErrorKind::Disconnected, "{ch}");
        b.plugin.ok("close", json!({ "path": ch }));
    }
    b.plugged();
    if !b.answers(&path, BOOT) {
        b.say("no answer to plain AT: sending CLD");
        let cld = [0xF9, 0x03, 0xEF, 0x05, 0xC3, 0x01, 0xF2, 0xF9];
        b.plugin
            .ok("write_binary", json!({ "path": path, "value": cld }));
        assert!(b.answers(&path, BOOT), "the modem answers after CLD");
    }
    b.say("plain AT answered");
    for ch in enable(b) {
        assert!(b.answers(&ch, BOOT), "{ch}");
    }
    b.say("3/3 ok, CMUX runs again");
}
