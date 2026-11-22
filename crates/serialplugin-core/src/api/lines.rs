//! Which ports have real modem control lines.

use crate::api::SerialManager;
use crate::cmux::parse_mux_path;
use crate::port::tcp::TCP_SCHEME;

impl SerialManager {
    /// `false` when `path` has no real RTS/DTR/CTS/DSR/RI/DCD: `tcp://` bridges and
    /// `ble://` UARTs, where the modem-line setters still succeed (the value is stored
    /// locally) and the getters return fixed values, and CMUX channels
    /// (`path#dlci=N`), where line calls fail (the lines belong to the physical
    /// port). Check this before relying on hardware flow control or line signalling.
    /// Local serial ports return `true`. Needs no open port.
    pub fn supports_modem_lines(&self, path: &str) -> bool {
        !(path.starts_with(TCP_SCHEME)
            || path.starts_with("ble://")
            || parse_mux_path(path).is_some())
    }
}
