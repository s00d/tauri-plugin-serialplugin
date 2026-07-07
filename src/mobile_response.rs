//! JSON response shapes returned by the Android Kotlin plugin.
//!
//! Kept separate from [`crate::mobile_api`] so serde roundtrips can be unit-tested on desktop CI.

use crate::error::Error;
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize, Debug)]
pub struct ManagedPortsResponse {
    pub ports: Vec<String>,
}

#[derive(Deserialize, Debug)]
pub struct StateResponse {
    pub state: bool,
}

#[derive(Deserialize, Debug)]
pub struct BytesResponse {
    pub bytes: u32,
}

#[derive(Deserialize, Debug)]
pub struct PathResponse {
    pub path: String,
}

pub fn parse_managed_ports(value: Value) -> Result<Vec<String>, Error> {
    if let Ok(response) = serde_json::from_value::<ManagedPortsResponse>(value.clone()) {
        return Ok(response.ports);
    }
    // Legacy / mistaken shape: map keys (incorrect but tolerate empty object)
    if let Value::Object(map) = value {
        if map.contains_key("ports") {
            return Err(Error::new("Invalid managed ports response format"));
        }
        return Ok(map.keys().cloned().collect());
    }
    Err(Error::new("Invalid response format"))
}

pub fn parse_state_response(value: Value) -> Result<bool, Error> {
    if let Ok(response) = serde_json::from_value::<StateResponse>(value.clone()) {
        return Ok(response.state);
    }
    if let Value::Bool(state) = value {
        return Ok(state);
    }
    Err(Error::new("Invalid response format"))
}

pub fn parse_bytes_response(value: Value) -> Result<u32, Error> {
    if let Ok(response) = serde_json::from_value::<BytesResponse>(value.clone()) {
        return Ok(response.bytes);
    }
    if let Value::Number(n) = value {
        return Ok(n.as_u64().unwrap_or(0) as u32);
    }
    Err(Error::new("Invalid response format"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn managed_ports_array_shape() {
        let json = serde_json::json!({ "ports": ["/dev/bus/usb/001/002", "/dev/bus/usb/001/003"] });
        let ports = parse_managed_ports(json).unwrap();
        assert_eq!(ports.len(), 2);
        assert_eq!(ports[0], "/dev/bus/usb/001/002");
    }

    #[test]
    fn state_wrapped_bool() {
        let json = serde_json::json!({ "state": true });
        assert!(parse_state_response(json).unwrap());
    }

    #[test]
    fn state_bare_bool() {
        assert!(!parse_state_response(serde_json::json!(false)).unwrap());
    }

    #[test]
    fn bytes_wrapped_number() {
        let json = serde_json::json!({ "bytes": 42 });
        assert_eq!(parse_bytes_response(json).unwrap(), 42);
    }

    #[test]
    fn bytes_bare_number() {
        assert_eq!(parse_bytes_response(serde_json::json!(7)).unwrap(), 7);
    }
}
