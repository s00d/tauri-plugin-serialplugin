//! [`SerialManager::available_ports_detailed`].

use crate::error::Error;
use crate::port::details::PortDetails;
use crate::SerialManager;

impl SerialManager {
    /// Every serial port with typed details (numeric vid/pid, USB interface number,
    /// location, Linux `/dev/serial` links), sorted by path. The 4.x
    /// [`Self::available_ports`] map is unchanged. On Android the fields come from
    /// the 4.x map (no interface / location).
    pub fn available_ports_detailed(&self) -> Result<Vec<PortDetails>, Error> {
        #[cfg(not(target_os = "android"))]
        {
            crate::port::details::available_ports_detailed()
        }
        #[cfg(target_os = "android")]
        {
            let mut out: Vec<PortDetails> = self
                .available_ports(false)?
                .iter()
                .map(|(path, info)| crate::port::details::details_from_info_map(path.clone(), info))
                .collect();
            out.sort_by(|a, b| a.path.cmp(&b.path));
            Ok(out)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::SerialManager;

    #[test]
    fn manager_lists_detailed_ports() {
        let details = SerialManager::new().available_ports_detailed().unwrap();
        let mut paths: Vec<_> = details.iter().map(|d| d.path.clone()).collect();
        let sorted = {
            let mut p = paths.clone();
            p.sort();
            p
        };
        assert_eq!(paths, sorted);
        paths.dedup();
        assert_eq!(paths.len(), details.len());
    }
}
