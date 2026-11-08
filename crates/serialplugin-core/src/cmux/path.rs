//! Composite path helpers for CMUX virtual channels.

/// Build a managed virtual port path for `dlci` on `physical`.
pub fn mux_path(physical: &str, dlci: u8) -> String {
    format!("{physical}#dlci={dlci}")
}

/// Data DLCIs are 1..=63 (6-bit address field; 0 is the control channel).
pub fn is_data_dlci(dlci: u8) -> bool {
    (1..=63).contains(&dlci)
}

/// Parse `physical#dlci=N` → `(physical, dlci)`.
pub fn parse_mux_path(path: &str) -> Option<(&str, u8)> {
    let (base, dlci_str) = path.rsplit_once("#dlci=")?;
    dlci_str
        .parse()
        .ok()
        .filter(|&d| is_data_dlci(d))
        .map(|dlci| (base, dlci))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mux_path_roundtrip() {
        let p = mux_path("/dev/ttyUSB0", 1);
        assert_eq!(parse_mux_path(&p), Some(("/dev/ttyUSB0", 1)));
        assert_eq!(parse_mux_path("/dev/ttyUSB0#dlci=0"), None);
        assert_eq!(parse_mux_path("/dev/ttyUSB0#dlci=65"), None);
    }
}
