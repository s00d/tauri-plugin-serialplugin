use crate::error::Error;
use crate::state::{ClearBuffer, DataBits, FlowControl, Parity, StopBits};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;
use tauri::plugin::PluginHandle;
use tauri::Runtime;

/// Access to the serial port APIs for mobile platforms.
pub struct SerialPort<R: Runtime>(pub PluginHandle<R>);

#[derive(Debug, Serialize, Deserialize)]
struct MobileResponse<T> {
    success: bool,
    data: Option<T>,
    error: Option<String>,
}

#[derive(Deserialize, Debug)]
struct PortInfo {
    #[serde(rename = "type")]
    type_: String,
    vid: String,
    pid: String,
    manufacturer: String,
    product: String,
    serial_number: String,
}

#[derive(Deserialize, Debug)]
struct AvailablePortsResponse {
    ports: HashMap<String, PortInfo>,
}

impl<R: Runtime> SerialPort<R> {
    /// Lists all available serial ports
    pub fn available_ports(&self) -> Result<HashMap<String, HashMap<String, String>>, Error> {
        let response: AvailablePortsResponse = self
            .0
            .run_mobile_plugin::<AvailablePortsResponse>("availablePorts", ())
            .map_err(|e| Error::String(e.to_string()))?;

        let mut result_list: HashMap<String, HashMap<String, String>> = HashMap::new();

        for (port_name, port_info) in response.ports {
            let mut port_map = HashMap::new();
            port_map.insert("type".to_string(), port_info.type_);
            port_map.insert("vid".to_string(), port_info.vid);
            port_map.insert("pid".to_string(), port_info.pid);
            port_map.insert("manufacturer".to_string(), port_info.manufacturer);
            port_map.insert("product".to_string(), port_info.product);
            port_map.insert("serial_number".to_string(), port_info.serial_number);

            result_list.insert(port_name, port_map);
        }

        Ok(result_list)
    }

    /// Lists all available serial ports using direct system commands
    pub fn available_ports_direct(
        &self,
    ) -> Result<HashMap<String, HashMap<String, String>>, Error> {
        match self.0.run_mobile_plugin("availablePortsDirect", ()) {
            Ok(Value::Object(result)) => serde_json::from_value(Value::Object(result))
                .map_err(|e| Error::String(format!("Failed to parse ports: {}", e))),
            Ok(_) => Err(Error::String("Invalid response format".to_string())),
            Err(e) => Err(Error::String(format!("Plugin error: {}", e))),
        }
    }

    /// Lists all managed serial ports (ports that are currently open and managed by the application).
    pub fn managed_ports(&self) -> Result<Vec<String>, Error> {
        let result = self.0.run_mobile_plugin("managedPorts", ());

        match result {
            Ok(Value::Object(result)) => {
                let port_list: Vec<String> = result.keys().cloned().collect();
                Ok(port_list)
            }
            Ok(_) => Err(Error::String("Invalid response format".to_string())), // Если не объект
            Err(e) => Err(Error::String(format!("Plugin error: {}", e))), // Ошибка плагина
        }
    }

    /// Opens a serial port with the specified settings
    pub fn open(
        &self,
        path: String,
        baud_rate: u32,
        data_bits: Option<DataBits>,
        flow_control: Option<FlowControl>,
        parity: Option<Parity>,
        stop_bits: Option<StopBits>,
        timeout: Option<u64>,
    ) -> Result<(), Error> {
        let params = serde_json::json!({
            "path": path,
            "baudRate": baud_rate,
            "dataBits": data_bits.unwrap_or(DataBits::Eight).as_u8(),
            "flowControl": flow_control.unwrap_or(FlowControl::None).as_u8(),
            "parity": parity.unwrap_or(Parity::None).as_u8(),
            "stopBits": stop_bits.unwrap_or(StopBits::One).as_u8(),
            "timeout": timeout.unwrap_or(1000),
        });

        match self.0.run_mobile_plugin("open", params) {
            Ok(Value::Bool(true)) => Ok(()),
            Ok(_) => Err(Error::String("Failed to open port".to_string())),
            Err(e) => Err(Error::String(format!("Plugin error: {}", e))),
        }
    }

    /// Closes a serial port
    pub fn close(&self, path: String) -> Result<(), Error> {
        let params = serde_json::json!({ "path": path });
        let response: MobileResponse<bool> = self
            .0
            .run_mobile_plugin::<MobileResponse<bool>>("close", params)?;
        match response.data {
            Some(true) => Ok(()),
            _ => Err(Error::String(
                response
                    .error
                    .unwrap_or_else(|| "Failed to close port".to_string()),
            )),
        }
    }

    /// Closes all open serial ports
    pub fn close_all(&self) -> Result<(), Error> {
        let response: MobileResponse<bool> = self
            .0
            .run_mobile_plugin::<MobileResponse<bool>>("closeAll", ())?;
        match response.data {
            Some(true) => Ok(()),
            _ => Err(Error::String(
                response
                    .error
                    .unwrap_or_else(|| "Failed to close all ports".to_string()),
            )),
        }
    }

    /// Force closes a serial port
    pub fn force_close(&self, path: String) -> Result<(), Error> {
        let params = serde_json::json!({ "path": path });
        let response: MobileResponse<bool> = self
            .0
            .run_mobile_plugin::<MobileResponse<bool>>("forceClose", params)?;
        match response.data {
            Some(true) => Ok(()),
            _ => {
                Err(Error::String(response.error.unwrap_or_else(|| {
                    "Failed to force close port".to_string()
                })))
            }
        }
    }

    /// Writes data to the serial port
    pub fn write(&self, path: String, data: String) -> Result<usize, Error> {
        let params = serde_json::json!({
            "path": path,
            "value": data,
        });

        match self.0.run_mobile_plugin("write", params) {
            Ok(Value::Number(n)) => Ok(n.as_u64().unwrap_or(0) as usize),
            Ok(_) => Err(Error::String("Invalid response format".to_string())),
            Err(e) => Err(Error::String(format!("Plugin error: {}", e))),
        }
    }

    /// Writes binary data to the serial port
    pub fn write_binary(&self, path: String, data: Vec<u8>) -> Result<usize, Error> {
        let params = serde_json::json!({
            "path": path,
            "value": data,
        });

        match self.0.run_mobile_plugin("writeBinary", params) {
            Ok(Value::Number(n)) => Ok(n.as_u64().unwrap_or(0) as usize),
            Ok(_) => Err(Error::String("Invalid response format".to_string())),
            Err(e) => Err(Error::String(format!("Plugin error: {}", e))),
        }
    }

    /// Reads data from the serial port
    pub fn read(
        &self,
        path: String,
        timeout: Option<u64>,
        size: Option<usize>,
    ) -> Result<String, Error> {
        let params = serde_json::json!({
            "path": path,
            "timeout": timeout.unwrap_or(1000),
            "size": size.unwrap_or(1024),
        });

        match self.0.run_mobile_plugin("read", params) {
            Ok(Value::String(data)) => Ok(data),
            Ok(_) => Err(Error::String("Invalid response format".to_string())),
            Err(e) => Err(Error::String(format!("Plugin error: {}", e))),
        }
    }

    /// Starts listening for data on the serial port
    pub fn start_listening(
        &self,
        path: String,
        timeout: Option<u64>,
        size: Option<usize>,
    ) -> Result<(), Error> {
        let params = serde_json::json!({ "path": path, "timeout": timeout, "size": size });
        let response: MobileResponse<bool> = self.0.run_mobile_plugin("startListening", params)?;
        match response.data {
            Some(true) => Ok(()),
            _ => Err(Error::String(
                response
                    .error
                    .unwrap_or_else(|| "Failed to start listening".to_string()),
            )),
        }
    }

    /// Stops listening for data on the serial port
    pub fn stop_listening(&self, path: String) -> Result<(), Error> {
        let params = serde_json::json!({ "path": path });
        let response: MobileResponse<bool> = self.0.run_mobile_plugin("stopListening", params)?;
        match response.data {
            Some(true) => Ok(()),
            _ => Err(Error::String(
                response
                    .error
                    .unwrap_or_else(|| "Failed to stop listening".to_string()),
            )),
        }
    }

    /// Sets the baud rate for the serial port
    pub fn set_baud_rate(&self, path: String, baud_rate: u32) -> Result<(), Error> {
        let params = serde_json::json!({
            "path": path,
            "baudRate": baud_rate,
        });

        match self.0.run_mobile_plugin("setBaudRate", params) {
            Ok(Value::Bool(true)) => Ok(()),
            Ok(_) => Err(Error::String("Failed to set baud rate".to_string())),
            Err(e) => Err(Error::String(format!("Plugin error: {}", e))),
        }
    }

    /// Sets the data bits for the serial port
    pub fn set_data_bits(&self, path: String, data_bits: DataBits) -> Result<(), Error> {
        let params = serde_json::json!({
            "path": path,
            "dataBits": data_bits,
        });

        match self.0.run_mobile_plugin("setDataBits", params) {
            Ok(Value::Bool(true)) => Ok(()),
            Ok(_) => Err(Error::String("Failed to set data bits".to_string())),
            Err(e) => Err(Error::String(format!("Plugin error: {}", e))),
        }
    }

    /// Sets the flow control for the serial port
    pub fn set_flow_control(&self, path: String, flow_control: FlowControl) -> Result<(), Error> {
        let params = serde_json::json!({
            "path": path,
            "flowControl": flow_control,
        });

        match self.0.run_mobile_plugin("setFlowControl", params) {
            Ok(Value::Bool(true)) => Ok(()),
            Ok(_) => Err(Error::String("Failed to set flow control".to_string())),
            Err(e) => Err(Error::String(format!("Plugin error: {}", e))),
        }
    }

    /// Sets the parity for the serial port
    pub fn set_parity(&self, path: String, parity: Parity) -> Result<(), Error> {
        let params = serde_json::json!({
            "path": path,
            "parity": parity,
        });

        match self.0.run_mobile_plugin("setParity", params) {
            Ok(Value::Bool(true)) => Ok(()),
            Ok(_) => Err(Error::String("Failed to set parity".to_string())),
            Err(e) => Err(Error::String(format!("Plugin error: {}", e))),
        }
    }

    /// Sets the stop bits for the serial port
    pub fn set_stop_bits(&self, path: String, stop_bits: StopBits) -> Result<(), Error> {
        let params = serde_json::json!({
            "path": path,
            "stopBits": stop_bits,
        });

        match self.0.run_mobile_plugin("setStopBits", params) {
            Ok(Value::Bool(true)) => Ok(()),
            Ok(_) => Err(Error::String("Failed to set stop bits".to_string())),
            Err(e) => Err(Error::String(format!("Plugin error: {}", e))),
        }
    }

    /// Sets the timeout for the serial port
    pub fn set_timeout(&self, path: String, timeout: Duration) -> Result<(), Error> {
        let params = serde_json::json!({
            "path": path,
            "timeout": timeout.as_millis(),
        });

        match self.0.run_mobile_plugin("setTimeout", params) {
            Ok(Value::Bool(true)) => Ok(()),
            Ok(_) => Err(Error::String("Failed to set timeout".to_string())),
            Err(e) => Err(Error::String(format!("Plugin error: {}", e))),
        }
    }

    /// Sets the RTS (Request To Send) signal
    pub fn write_request_to_send(&self, path: String, level: bool) -> Result<(), Error> {
        let params = serde_json::json!({
            "path": path,
            "level": level,
        });

        match self.0.run_mobile_plugin("writeRequestToSend", params) {
            Ok(Value::Bool(true)) => Ok(()),
            Ok(_) => Err(Error::String("Failed to set RTS".to_string())),
            Err(e) => Err(Error::String(format!("Plugin error: {}", e))),
        }
    }

    /// Sets the DTR (Data Terminal Ready) signal
    pub fn write_data_terminal_ready(&self, path: String, level: bool) -> Result<(), Error> {
        let params = serde_json::json!({
            "path": path,
            "level": level,
        });

        match self.0.run_mobile_plugin("writeDataTerminalReady", params) {
            Ok(Value::Bool(true)) => Ok(()),
            Ok(_) => Err(Error::String("Failed to set DTR".to_string())),
            Err(e) => Err(Error::String(format!("Plugin error: {}", e))),
        }
    }

    pub fn cancel_read(&self, path: String) -> Result<(), Error> {
        let params = serde_json::json!({
            "path": path,
        });

        match self.0.run_mobile_plugin("cancelRead", params) {
            Ok(Value::Bool(true)) => Ok(()),
            Ok(_) => Err(Error::String("Failed to cancel read".to_string())),
            Err(e) => Err(Error::String(format!("Plugin error: {}", e))),
        }
    }

    /// Reads the CTS (Clear To Send) signal state
    pub fn read_clear_to_send(&self, path: String) -> Result<bool, Error> {
        let params = serde_json::json!({ "path": path });
        match self.0.run_mobile_plugin("readClearToSend", params) {
            Ok(Value::Bool(state)) => Ok(state),
            Ok(_) => Err(Error::String("Invalid response format".to_string())),
            Err(e) => Err(Error::String(format!("Plugin error: {}", e))),
        }
    }

    /// Reads the DSR (Data Set Ready) signal state
    pub fn read_data_set_ready(&self, path: String) -> Result<bool, Error> {
        let params = serde_json::json!({ "path": path });
        match self.0.run_mobile_plugin("readDataSetReady", params) {
            Ok(Value::Bool(state)) => Ok(state),
            Ok(_) => Err(Error::String("Invalid response format".to_string())),
            Err(e) => Err(Error::String(format!("Plugin error: {}", e))),
        }
    }

    /// Reads the RI (Ring Indicator) signal state
    pub fn read_ring_indicator(&self, path: String) -> Result<bool, Error> {
        let params = serde_json::json!({ "path": path });
        match self.0.run_mobile_plugin("readRingIndicator", params) {
            Ok(Value::Bool(state)) => Ok(state),
            Ok(_) => Err(Error::String("Invalid response format".to_string())),
            Err(e) => Err(Error::String(format!("Plugin error: {}", e))),
        }
    }

    /// Reads the CD (Carrier Detect) signal state
    pub fn read_carrier_detect(&self, path: String) -> Result<bool, Error> {
        let params = serde_json::json!({ "path": path });
        match self.0.run_mobile_plugin("readCarrierDetect", params) {
            Ok(Value::Bool(state)) => Ok(state),
            Ok(_) => Err(Error::String("Invalid response format".to_string())),
            Err(e) => Err(Error::String(format!("Plugin error: {}", e))),
        }
    }

    /// Gets the number of bytes available to read
    pub fn bytes_to_read(&self, path: String) -> Result<u32, Error> {
        let params = serde_json::json!({ "path": path });
        match self.0.run_mobile_plugin("bytesToRead", params) {
            Ok(Value::Number(n)) => Ok(n.as_u64().unwrap_or(0) as u32),
            Ok(_) => Err(Error::String("Invalid response format".to_string())),
            Err(e) => Err(Error::String(format!("Plugin error: {}", e))),
        }
    }

    /// Gets the number of bytes waiting to be written
    pub fn bytes_to_write(&self, path: String) -> Result<u32, Error> {
        let params = serde_json::json!({ "path": path });
        match self.0.run_mobile_plugin("bytesToWrite", params) {
            Ok(Value::Number(n)) => Ok(n.as_u64().unwrap_or(0) as u32),
            Ok(_) => Err(Error::String("Invalid response format".to_string())),
            Err(e) => Err(Error::String(format!("Plugin error: {}", e))),
        }
    }

    /// Clears the specified buffer
    pub fn clear_buffer(&self, path: String, buffer_type: ClearBuffer) -> Result<(), Error> {
        let params = serde_json::json!({
            "path": path,
            "bufferType": buffer_type,
        });

        match self.0.run_mobile_plugin("clearBuffer", params) {
            Ok(Value::Bool(true)) => Ok(()),
            Ok(_) => Err(Error::String("Failed to clear buffer".to_string())),
            Err(e) => Err(Error::String(format!("Plugin error: {}", e))),
        }
    }

    /// Sets the break signal
    pub fn set_break(&self, path: String) -> Result<(), Error> {
        let params = serde_json::json!({ "path": path });
        match self.0.run_mobile_plugin("setBreak", params) {
            Ok(Value::Bool(true)) => Ok(()),
            Ok(_) => Err(Error::String("Failed to set break".to_string())),
            Err(e) => Err(Error::String(format!("Plugin error: {}", e))),
        }
    }

    /// Clears the break signal
    pub fn clear_break(&self, path: String) -> Result<(), Error> {
        let params = serde_json::json!({ "path": path });
        match self.0.run_mobile_plugin("clearBreak", params) {
            Ok(Value::Bool(true)) => Ok(()),
            Ok(_) => Err(Error::String("Failed to clear break".to_string())),
            Err(e) => Err(Error::String(format!("Plugin error: {}", e))),
        }
    }
}
