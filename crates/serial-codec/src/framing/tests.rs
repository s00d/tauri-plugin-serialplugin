//! Only what the public API cannot observe: the regex framer's rescan bookkeeping.
//! Everything else is tested through `FramerConfig` in `tests/framing*.rs`.

use super::*;

/// The rescan-from-start rule is private state (`full_scan`): output is the same with a
/// quadratic rule, only slower, and wall-clock checks flake on a loaded host.
#[test]
fn regex_without_matches_is_linear() {
    let mut p = RegexFramer {
        regex: regex_lite::Regex::new("\r\n").unwrap(),
        max: 4096,
        text: String::new(),
        pending: Vec::new(),
        scanned: 0,
        full_scan: 0,
    };
    let t = Instant::now();
    let mut out = Vec::new();
    for _ in 0..8 << 10 {
        p.push(b"x", t, &mut out);
        // Searched from the start only each time the text doubled: checked per
        // push, so a rescan rule gone quadratic fails at once, not after minutes.
        let len = p.text.len();
        let doubled = if len == 0 { 0 } else { 1 << len.ilog2() };
        assert_eq!(p.full_scan, doubled, "text of {len} bytes");
    }
    // Only the maxBufferSize cuts.
    // No wall-clock limit: the per-push `full_scan` check pins linearity, and a
    // timer only flakes on a loaded host.
    assert_eq!(out.len(), 2);
}
