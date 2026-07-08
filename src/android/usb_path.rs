//! Session path parsing (`deviceName` or `deviceName#port`).

pub fn parse(path: &str) -> (&str, usize) {
    if let Some(idx) = path.rfind('#') {
        let device = &path[..idx];
        let port = path[idx + 1..].parse().unwrap_or(0);
        (device, port)
    } else {
        (path, 0)
    }
}

pub fn session_key(device_name: &str, port_index: usize, port_count: usize) -> String {
    if port_count <= 1 {
        device_name.to_string()
    } else {
        format!("{device_name}#{port_index}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single() {
        assert_eq!(parse("/dev/bus/usb/001/002"), ("/dev/bus/usb/001/002", 0));
    }

    #[test]
    fn parse_multi() {
        assert_eq!(parse("/dev/bus/usb/001/002#1"), ("/dev/bus/usb/001/002", 1));
    }
}
