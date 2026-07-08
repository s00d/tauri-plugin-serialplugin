//! Software XON/XOFF filter for RX paths.

use crate::config::{CHAR_XOFF, CHAR_XON};

#[derive(Debug, Clone, Default)]
pub struct XonXoffFilter {
    pub enabled: bool,
    pub paused: bool,
}

impl XonXoffFilter {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            paused: false,
        }
    }

    /// Strip inline XON/XOFF control bytes (matches Java `XonXoffFilter`).
    pub fn filter(&mut self, input: &[u8]) -> Vec<u8> {
        if !self.enabled {
            return input.to_vec();
        }
        let ctrl_count = input
            .iter()
            .filter(|&&b| b == CHAR_XON || b == CHAR_XOFF)
            .count();
        if ctrl_count == 0 {
            return input.to_vec();
        }
        let mut out = Vec::with_capacity(input.len().saturating_sub(ctrl_count));
        for &b in input {
            match b {
                CHAR_XON => self.paused = false,
                CHAR_XOFF => self.paused = true,
                _ => out.push(b),
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_between_xoff_and_xon() {
        let mut f = XonXoffFilter::new(true);
        let out = f.filter(&[b'a', CHAR_XOFF, b'b', CHAR_XON, b'c']);
        assert_eq!(out, vec![b'a', b'b', b'c']);
    }
}
