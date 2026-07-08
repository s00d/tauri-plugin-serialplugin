//! Composite path helpers for CMUX virtual channels.

/// Build a managed virtual port path for `dlci` on `physical`.
pub fn mux_path(physical: &str, dlci: u8) -> String {
    format!("{physical}#dlci={dlci}")
}

/// Parse `physical#dlci=N` → `(physical, dlci)`.
pub fn parse_mux_path(path: &str) -> Option<(&str, u8)> {
    let (base, dlci_str) = path.rsplit_once("#dlci=")?;
    dlci_str.parse().ok().map(|dlci| (base, dlci))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mux_path_roundtrip() {
        let p = mux_path("/dev/ttyUSB0", 1);
        assert_eq!(parse_mux_path(&p), Some(("/dev/ttyUSB0", 1)));
    }
}
