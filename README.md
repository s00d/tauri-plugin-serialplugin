[![npm version](https://img.shields.io/npm/v/tauri-plugin-serialplugin/latest?style=for-the-badge)](https://www.npmjs.com/package/tauri-plugin-serialplugin)
[![Crates.io](https://img.shields.io/crates/v/tauri-plugin-serialplugin?style=for-the-badge)](https://crates.io/crates/tauri-plugin-serialplugin)
[![Documentation](https://img.shields.io/badge/docs-docs.rs-blue?style=for-the-badge)](https://docs.rs/tauri-plugin-serialplugin/latest/tauri_plugin_serialplugin/all.html)
[![GitHub issues](https://img.shields.io/github/issues/s00d/tauri-plugin-serialplugin?style=for-the-badge)](https://github.com/s00d/tauri-plugin-serialplugin/issues)
[![GitHub stars](https://img.shields.io/github/stars/s00d/tauri-plugin-serialplugin?style=for-the-badge)](https://github.com/s00d/tauri-plugin-serialplugin/stargazers)
[![Donate](https://img.shields.io/badge/Donate-Donationalerts-ff4081?style=for-the-badge)](https://www.donationalerts.com/r/s00d88)

# Tauri Plugin — SerialPort

A comprehensive plugin for Tauri applications to communicate with serial ports. This plugin provides a complete API for reading from and writing to serial devices, with support for various configuration options and control signals.

> **⚠️ Important Notice:** The JavaScript dependency has been renamed from `tauri-plugin-serialplugin` to `tauri-plugin-serialplugin-api`. Please update your `package.json` before updating to the latest version, following the same pattern used by other Tauri plugins.

---

## Table of Contents

1. [Installation](#installation)
2. [Basic Usage](#basic-usage)
3. [TypeScript Support](#typescript-support)
4. [Rust Usage](#rust-usage)
5. [Permissions](#permissions)
6. [API Reference](#api-reference)  
   6.1. [Port Discovery](#port-discovery)  
   6.2. [Connection Management](#connection-management)  
   6.3. [Data Transfer](#data-transfer)  
   6.4. [Port Configuration](#port-configuration)  
   6.5. [Control Signals](#control-signals)  
   6.6. [Buffer Management](#buffer-management)
7. [Common Use Cases](#common-use-cases)
8. [Android Setup](#android-setup)
9. [Contributing](#contributing)
10. [Development Setup](#development-setup)
11. [Testing](#testing)
12. [Partners](#partners)
13. [License](#license)

---

## Installation

### Prerequisites

- **Rust** version 1.70 or higher
- **Tauri** 2.0 or higher
- **Node.js** and an npm-compatible package manager (npm, yarn, pnpm)




## Installation

### Automatic Installation (Recommended)

Use the Tauri CLI to automatically install both the Rust and JavaScript parts of the plugin:

```bash
# npm
npm run tauri add serialplugin

# yarn  
yarn run tauri add serialplugin

# pnpm
pnpm tauri add serialplugin

# deno
deno task tauri add serialplugin

# bun
bun tauri add serialplugin

# cargo
cargo tauri add serialplugin
```

### Manual Installation

#### Backend (Rust)

Add the plugin using cargo:

```bash
cd ./src-tauri
cargo add tauri-plugin-serialplugin
```

#### Frontend (JavaScript/TypeScript)

Install the JavaScript API:

```bash
npm install tauri-plugin-serialplugin-api
# or
pnpm add tauri-plugin-serialplugin-api
```

---

## Basic Usage

1. **Register the Plugin**
   ```rust
   // src-tauri/src/main.rs
   fn main() {
       tauri::Builder::default()
           .plugin(tauri_plugin_serialplugin::init())
           .run(tauri::generate_context!())
           .expect("error while running tauri application");
   }
   ```

2. **Configure Permissions**
   ```jsonc
   // src-tauri/capabilities/default.json
   {
     "$schema": "../gen/schemas/desktop-schema.json",
     "identifier": "default",
     "description": "Capability for the main window",
     "windows": ["main"],
     "permissions": [
       "core:default",
       "serialplugin:default"
     ]
   }
   ```

3. **Basic Example**
   ```typescript
   import { SerialPort } from "tauri-plugin-serialplugin-api";

   // List available ports
   const ports = await SerialPort.available_ports();
   console.log("Available ports:", ports);

   // Open a port
   const port = new SerialPort({
     path: "COM1",
     baudRate: 9600
   });
   await port.open();

   // Write data
   await port.write("Hello, Serial Port!");

   // Start port listening
   const unsubscribe = await port.listen((data) => {
     console.log("Received:", data);
   });

   // Stop listening when done
   await port.cancelListen();

   // Close port
   await port.close();
   ```

4. **Error Handling Example**
   ```typescript
   import { SerialPort } from "tauri-plugin-serialplugin-api";

   async function handleSerialPort() {
     let port: SerialPort | null = null;

     try {
       // List available ports
       const ports = await SerialPort.available_ports();
       if (Object.keys(ports).length === 0) {
         throw new Error("No serial ports found");
       }

       // Open port
       port = new SerialPort({
         path: "COM1",
         baudRate: 9600
       });

       try {
         await port.open();
       } catch (error) {
         throw new Error(`Failed to open port: ${error}`);
       }

       try {
         // Write data
         await port.write("Test data");
       } catch (error) {
         throw new Error(`Failed to write data: ${error}`);
       }

       try {
         // Read data
         const data = await port.read({ timeout: 1000 });
         console.log("Received:", data);
       } catch (error) {
         throw new Error(`Failed to read data: ${error}`);
       }

       try {
         // Start listening
         await port.startListening();
         await port.listen((data) => {
           console.log("Received:", data);
         });
       } catch (error) {
         throw new Error(`Failed to start listening: ${error}`);
       }

       try {
         // Configure port settings
         await port.setBaudRate(115200);
         await port.setDataBits(DataBits.Eight);
         await port.setFlowControl(FlowControl.None);
         await port.setParity(Parity.None);
         await port.setStopBits(StopBits.One);
         await port.setTimeout(1000);
       } catch (error) {
         throw new Error(`Failed to configure port: ${error}`);
       }

     } catch (error) {
       // Handle all errors in one place
       console.error("Serial port error:", error);
     } finally {
       // Clean up
       if (port) {
         try {
           await port.cancelListen();
           await port.close();
         } catch (error) {
           console.error("Error during cleanup:", error);
         }
       }
     }
   }

   // Usage
   handleSerialPort();
   ```

---

## TypeScript Support

This plugin provides full TypeScript support with comprehensive type definitions. All methods, interfaces, and enums are properly typed for better development experience.

### Available Types

```typescript
import { 
  SerialPort, 
  DataBits, 
  FlowControl, 
  Parity, 
  StopBits, 
  ClearBuffer,
  PortInfo,
  SerialportOptions,
  ReadOptions 
} from "tauri-plugin-serialplugin-api";
```

### Type Definitions

- **`SerialPort`** - Main class for serial port operations
- **`DataBits`** - Enum: `Five`, `Six`, `Seven`, `Eight`
- **`FlowControl`** - Enum: `None`, `Software`, `Hardware`
- **`Parity`** - Enum: `None`, `Odd`, `Even`
- **`StopBits`** - Enum: `One`, `Two`
- **`ClearBuffer`** - Enum: `Input`, `Output`, `All`
- **`PortInfo`** - Interface for port information
- **`SerialportOptions`** - Interface for port configuration
- **`ReadOptions`** - Interface for read operation options

### Configuration Example with Types

```typescript
import { SerialPort, DataBits, FlowControl, Parity, StopBits } from "tauri-plugin-serialplugin-api";

const port = new SerialPort({
  path: "/dev/ttyUSB0",
  baudRate: 9600,
  dataBits: DataBits.Eight,        // Type-safe enum
  flowControl: FlowControl.None,   // Type-safe enum
  parity: Parity.None,             // Type-safe enum
  stopBits: StopBits.One,          // Type-safe enum
  timeout: 1000,
  size: 1024
});

// All configuration methods are fully typed
await port.setBaudRate(115200);
await port.setDataBits(DataBits.Eight);
await port.setFlowControl(FlowControl.None);
await port.setParity(Parity.None);
await port.setStopBits(StopBits.One);
await port.setTimeout(500);
```

### Control Signals with Types

```typescript
// Set control signals
await port.setRequestToSend(true);
await port.setDataTerminalReady(true);

// Alternative methods (writeRequestToSend and writeDataTerminalReady)
await port.writeRequestToSend(true);
await port.writeDataTerminalReady(true);

// Read control signals
const cts = await port.readClearToSend();
const dsr = await port.readDataSetReady();
const ri = await port.readRingIndicator();
const cd = await port.readCarrierDetect();
```

### Buffer Management with Types

```typescript
import { ClearBuffer } from "tauri-plugin-serialplugin-api";

// Check buffer status
const bytesToRead = await port.bytesToRead();
const bytesToWrite = await port.bytesToWrite();

// Clear buffers with type-safe enum
await port.clearBuffer(ClearBuffer.Input);
await port.clearBuffer(ClearBuffer.Output);
await port.clearBuffer(ClearBuffer.All);

// Break signal control
await port.setBreak();
await port.clearBreak();
```

---

## Rust Usage

This plugin can also be used directly from Rust code in your Tauri backend. For complete API documentation, see [docs.rs](https://docs.rs/tauri-plugin-serialplugin/).

Here's how to use it:

### Using Commands Directly

You can import and use the command functions directly from the plugin:

```rust
use tauri_plugin_serialplugin::commands::{
    available_ports, open, write, read, close, set_baud_rate,
    set_data_bits, set_flow_control, set_parity, set_stop_bits, set_timeout,
    write_request_to_send, write_data_terminal_ready,
    read_clear_to_send, read_data_set_ready,
    bytes_to_read, bytes_to_write, clear_buffer,
    set_break, clear_break
};
use tauri_plugin_serialplugin::state::{DataBits, FlowControl, Parity, StopBits, ClearBuffer};
use tauri::{AppHandle, State, Runtime};
use std::collections::HashMap;

#[tauri::command]
async fn rust_serial_example(
    app: AppHandle<tauri::Wry>,
    serial: State<'_, tauri_plugin_serialplugin::desktop_api::SerialPort<tauri::Wry>>
) -> Result<(), String> {
    // Get available ports
    let ports = available_ports(app.clone(), serial.clone())
        .map_err(|e| format!("Failed to get ports: {}", e))?;
    println!("Available ports: {:?}", ports);

    // Open a serial port
    let path = "COM1".to_string();
    let baud_rate = 9600;
    
    open(
        app.clone(),
        serial.clone(),
        path.clone(),
        baud_rate,
        Some(DataBits::Eight),
        Some(FlowControl::None),
        Some(Parity::None),
        Some(StopBits::One),
        Some(1000u64) // timeout in milliseconds
    ).map_err(|e| format!("Failed to open port: {}", e))?;

    // Write data
    let data = "Hello from Rust!".to_string();
    let bytes_written = write(app.clone(), serial.clone(), path.clone(), data)
        .map_err(|e| format!("Failed to write: {}", e))?;
    println!("Wrote {} bytes", bytes_written);

    // Read data
    let received_data = read(
        app.clone(),
        serial.clone(),
        path.clone(),
        Some(1000u64), // timeout
        Some(1024usize) // max bytes to read
    ).map_err(|e| format!("Failed to read: {}", e))?;
    println!("Received: {}", received_data);

    // Configure port settings
    set_baud_rate(app.clone(), serial.clone(), path.clone(), 115200)
        .map_err(|e| format!("Failed to set baud rate: {}", e))?;
    
    set_data_bits(app.clone(), serial.clone(), path.clone(), DataBits::Eight)
        .map_err(|e| format!("Failed to set data bits: {}", e))?;
    
    set_flow_control(app.clone(), serial.clone(), path.clone(), FlowControl::None)
        .map_err(|e| format!("Failed to set flow control: {}", e))?;
    
    set_parity(app.clone(), serial.clone(), path.clone(), Parity::None)
        .map_err(|e| format!("Failed to set parity: {}", e))?;
    
    set_stop_bits(app.clone(), serial.clone(), path.clone(), StopBits::One)
        .map_err(|e| format!("Failed to set stop bits: {}", e))?;

    // Set timeout
    set_timeout(app.clone(), serial.clone(), path.clone(), 1000u64)
        .map_err(|e| format!("Failed to set timeout: {}", e))?;

    // Control signals
    write_request_to_send(app.clone(), serial.clone(), path.clone(), true)
        .map_err(|e| format!("Failed to set RTS: {}", e))?;
    
    write_data_terminal_ready(app.clone(), serial.clone(), path.clone(), true)
        .map_err(|e| format!("Failed to set DTR: {}", e))?;

    // Read control signals
    let cts = read_clear_to_send(app.clone(), serial.clone(), path.clone())
        .map_err(|e| format!("Failed to read CTS: {}", e))?;
    println!("CTS: {}", cts);

    let dsr = read_data_set_ready(app.clone(), serial.clone(), path.clone())
        .map_err(|e| format!("Failed to read DSR: {}", e))?;
    println!("DSR: {}", dsr);

    // Buffer management
    let bytes_to_read = bytes_to_read(app.clone(), serial.clone(), path.clone())
        .map_err(|e| format!("Failed to get bytes to read: {}", e))?;
    println!("Bytes available to read: {}", bytes_to_read);

    let bytes_to_write = bytes_to_write(app.clone(), serial.clone(), path.clone())
        .map_err(|e| format!("Failed to get bytes to write: {}", e))?;
    println!("Bytes waiting to write: {}", bytes_to_write);

    // Clear buffers
    clear_buffer(app.clone(), serial.clone(), path.clone(), ClearBuffer::All)
        .map_err(|e| format!("Failed to clear buffer: {}", e))?;

    // Break signal
    set_break(app.clone(), serial.clone(), path.clone())
        .map_err(|e| format!("Failed to set break: {}", e))?;
    
    clear_break(app.clone(), serial.clone(), path.clone())
        .map_err(|e| format!("Failed to clear break: {}", e))?;

    // Close the port
    close(app, serial, path)
        .map_err(|e| format!("Failed to close port: {}", e))?;

    Ok(())
}
```

### Advanced Rust Example with Error Handling

```rust
use tauri_plugin_serialplugin::commands::{
    available_ports, open, write, read, close, force_close, managed_ports, start_listening
};
use tauri_plugin_serialplugin::state::{DataBits, FlowControl, Parity, StopBits};
use tauri::{AppHandle, State};
use std::collections::HashMap;

#[tauri::command]
async fn advanced_serial_example(
    app: AppHandle<tauri::Wry>,
    serial: State<'_, tauri_plugin_serialplugin::desktop_api::SerialPort<tauri::Wry>>
) -> Result<(), String> {
    // Get available ports with error handling
    let ports = match available_ports(app.clone(), serial.clone()) {
        Ok(ports) => ports,
        Err(e) => {
            eprintln!("Failed to get available ports: {}", e);
            return Err("No serial ports available".to_string());
        }
    };

    if ports.is_empty() {
        return Err("No serial ports found".to_string());
    }

    // Use the first available port
    let port_path = ports.keys().next().unwrap().clone();
    println!("Using port: {}", port_path);

    // Open port with full configuration
    let open_result = open(
        app.clone(),
        serial.clone(),
        port_path.clone(),
        9600u32, // baud rate
        Some(DataBits::Eight),
        Some(FlowControl::None),
        Some(Parity::None),
        Some(StopBits::One),
        Some(5000u64) // 5 second timeout
    );

    match open_result {
        Ok(_) => println!("Port opened successfully"),
        Err(e) => {
            eprintln!("Failed to open port: {}", e);
            return Err(format!("Failed to open port {}: {}", port_path, e));
        }
    }

    // Start listening for data
    match start_listening(
        app.clone(),
        serial.clone(),
        port_path.clone(),
        Some(1000u64), // timeout
        Some(1024usize) // max bytes
    ) {
        Ok(_) => println!("Started listening"),
        Err(e) => {
            eprintln!("Failed to start listening: {}", e);
            // Continue anyway, we can still read manually
        }
    }

    // Send a command and read response
    let command = "AT\r\n".to_string();
    match write(app.clone(), serial.clone(), port_path.clone(), command) {
        Ok(bytes) => println!("Sent {} bytes", bytes),
        Err(e) => {
            eprintln!("Failed to write command: {}", e);
            return Err(format!("Write failed: {}", e));
        }
    }

    // Read response with timeout
    match read(
        app.clone(),
        serial.clone(),
        port_path.clone(),
        Some(2000u64), // 2 second timeout
        Some(512usize) // max 512 bytes
    ) {
        Ok(response) => println!("Response: {}", response),
        Err(e) => {
            eprintln!("Failed to read response: {}", e);
            return Err(format!("Read failed: {}", e));
        }
    }

    // Get managed ports
    let managed_ports = match managed_ports(app.clone(), serial.clone()) {
        Ok(ports) => ports,
        Err(e) => {
            eprintln!("Failed to get managed ports: {}", e);
            Vec::new()
        }
    };
    println!("Managed ports: {:?}", managed_ports);

    // Clean up
    let cleanup_result = close(app.clone(), serial.clone(), port_path.clone());
    match cleanup_result {
        Ok(_) => println!("Port closed successfully"),
        Err(e) => {
            eprintln!("Failed to close port: {}", e);
            // Try force close
            if let Err(e2) = force_close(app, serial, port_path) {
                eprintln!("Failed to force close port: {}", e2);
            }
        }
    }

    Ok(())
}
```

### Binary Data Handling in Rust

```rust
use tauri_plugin_serialplugin::commands::{open, write_binary, read_binary, close};
use tauri_plugin_serialplugin::state::{DataBits, FlowControl, Parity, StopBits};
use tauri::{AppHandle, State};

#[tauri::command]
async fn binary_data_example(
    app: AppHandle<tauri::Wry>,
    serial: State<'_, tauri_plugin_serialplugin::desktop_api::SerialPort<tauri::Wry>>
) -> Result<(), String> {
    let port_path = "COM1".to_string();
    
    // Open port
    open(
        app.clone(),
        serial.clone(),
        port_path.clone(),
        115200u32,
        Some(DataBits::Eight),
        Some(FlowControl::None),
        Some(Parity::None),
        Some(StopBits::One),
        Some(1000u64)
    ).map_err(|e| format!("Failed to open port: {}", e))?;

    // Write binary data
    let binary_data = vec![0x48, 0x65, 0x6C, 0x6C, 0x6F]; // "Hello" in ASCII
    let bytes_written = write_binary(app.clone(), serial.clone(), port_path.clone(), binary_data)
        .map_err(|e| format!("Failed to write binary data: {}", e))?;
    println!("Wrote {} bytes of binary data", bytes_written);

    // Read binary data
    let received_data = read_binary(
        app.clone(),
        serial.clone(),
        port_path.clone(),
        Some(1000u64), // timeout
        Some(256usize) // max bytes
    ).map_err(|e| format!("Failed to read binary data: {}", e))?;
    
    println!("Received {} bytes: {:?}", received_data.len(), received_data);

    // Close port
    close(app, serial, port_path)
        .map_err(|e| format!("Failed to close port: {}", e))?;

    Ok(())
}
```

### Using Commands vs Direct API

You have two ways to use the plugin in Rust:

#### Option 1: Using Commands (Recommended)

Import and use the command functions directly. These functions are documented in the [docs.rs documentation](https://docs.rs/tauri-plugin-serialplugin/):

```rust
use tauri_plugin_serialplugin::commands::{available_ports, open, write, read, close};
use tauri::{AppHandle, State};

#[tauri::command]
async fn my_serial_function(
    app: AppHandle<tauri::Wry>,
    serial: State<'_, tauri_plugin_serialplugin::desktop_api::SerialPort<tauri::Wry>>
) -> Result<(), String> {
    // Use command functions
    let ports = available_ports(app.clone(), serial.clone())?;
    open(app.clone(), serial.clone(), "COM1".to_string(), 9600, None, None, None, None, None)?;
    // ... rest of your code
}
```

#### Option 2: Using Direct API

Use the SerialPort methods directly:

```rust
use tauri::State;
use tauri_plugin_serialplugin::desktop_api::SerialPort;

#[tauri::command]
async fn my_serial_function(
    serial: State<'_, SerialPort<tauri::Wry>>
) -> Result<(), String> {
    // Use serial methods directly
    let ports = serial.available_ports()?;
    // ... rest of your code
}
```

### Available Rust Types

The plugin provides the following Rust types for configuration:

```rust
use tauri_plugin_serialplugin::state::{
    DataBits,      // Five, Six, Seven, Eight
    FlowControl,   // None, Software, Hardware
    Parity,        // None, Odd, Even
    StopBits,      // One, Two
    ClearBuffer    // Input, Output, All
};
```

### Complete Command Functions Reference

Here are all the available command functions you can import and use. For detailed documentation with examples, see the [docs.rs documentation](https://docs.rs/tauri-plugin-serialplugin/):

```rust
use tauri_plugin_serialplugin::commands::{
    // Port discovery
    available_ports,           // Get list of available ports
    available_ports_direct,    // Get ports using platform-specific commands
    managed_ports,             // Get list of currently managed ports
    
    // Connection management
    open,                      // Open a serial port
    close,                     // Close a serial port
    close_all,                 // Close all open ports
    force_close,               // Force close a port
    
    // Data transfer
    write,                     // Write string data
    write_binary,              // Write binary data
    read,                      // Read string data
    read_binary,               // Read binary data
    
    // Listening
    start_listening,           // Start listening for data
    stop_listening,            // Stop listening
    cancel_read,               // Cancel read operations
    
    // Port configuration
    set_baud_rate,             // Set baud rate
    set_data_bits,             // Set data bits
    set_flow_control,          // Set flow control
    set_parity,                // Set parity
    set_stop_bits,             // Set stop bits
    set_timeout,               // Set timeout
    
    // Control signals
    write_request_to_send,     // Set RTS signal
    write_data_terminal_ready, // Set DTR signal
    read_clear_to_send,        // Read CTS signal
    read_data_set_ready,       // Read DSR signal
    read_ring_indicator,       // Read RI signal
    read_carrier_detect,       // Read CD signal
    
    // Buffer management
    bytes_to_read,             // Get bytes available to read
    bytes_to_write,            // Get bytes waiting to write
    clear_buffer,              // Clear buffers
    
    // Break signal
    set_break,                 // Start break signal
    clear_break,               // Stop break signal
};
```

### Command Function Signatures

All command functions follow this pattern:

```rust
pub fn function_name<R: Runtime>(
    app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    // ... additional parameters specific to the function
) -> Result<ReturnType, Error>
```

For example:
```rust
// Open port
pub fn open<R: Runtime>(
    app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    baud_rate: u32,
    data_bits: Option<DataBits>,
    flow_control: Option<FlowControl>,
    parity: Option<Parity>,
    stop_bits: Option<StopBits>,
    timeout: Option<u64>,
) -> Result<(), Error>

// Write data
pub fn write<R: Runtime>(
    app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    value: String,
) -> Result<usize, Error>
```

### Error Messages

#### Port Discovery
- "Failed to lock serialports mutex" - Error acquiring mutex lock when listing ports
- "Invalid response format" - Invalid response format from plugin
- "Plugin error: {error}" - Plugin execution error

#### Port Management
- "Failed to acquire lock: {error}" - Error acquiring mutex lock
- "Port '{path}' not found" - Port does not exist
- "Serial port {path} is not open!" - Port is not open
- "Failed to open serial port: {error}" - Error opening port
- "Failed to clone serial port: {error}" - Error cloning port
- "Failed to set short timeout: {error}" - Error setting timeout
- "Failed to stop existing listener: {error}" - Error stopping existing listener
- "Failed to join thread: {error}" - Error waiting for thread completion
- "Failed to cancel serial port data reading: {error}" - Error canceling data reading

#### Data Operations
- "Failed to write data: {error}" - Error writing data
- "Failed to write binary data: {error}" - Error writing binary data
- "Failed to read data: {error}" - Error reading data
- "no data received within {timeout} ms" - Read timeout
- "Failed to set timeout: {error}" - Error setting timeout

#### Port Configuration
- "Failed to set baud rate: {error}" - Error setting baud rate
- "Failed to set data bits: {error}" - Error setting data bits
- "Failed to set flow control: {error}" - Error setting flow control
- "Failed to set parity: {error}" - Error setting parity
- "Failed to set stop bits: {error}" - Error setting stop bits

#### Control Signals
- "Failed to set RTS: {error}" - Error setting RTS signal
- "Failed to set DTR: {error}" - Error setting DTR signal
- "Failed to read CTS: {error}" - Error reading CTS signal
- "Failed to read DSR: {error}" - Error reading DSR signal
- "Failed to read RI: {error}" - Error reading RI signal
- "Failed to read CD: {error}" - Error reading CD signal
- "Failed to set break: {error}" - Error setting break signal
- "Failed to clear break: {error}" - Error clearing break signal

#### Buffer Management
- "Failed to clear buffer: {error}" - Error clearing buffer
- "Failed to get bytes to read: {error}" - Error getting bytes available to read
- "Failed to get bytes to write: {error}" - Error getting bytes waiting to write

---

## Permissions

Below is a list of all permissions the plugin supports. Granting or denying them allows fine-grained control over what your application can do with serial ports.

| Permission                                  | Description                                                                   |
|---------------------------------------------|-------------------------------------------------------------------------------|
| `serialplugin:allow-available-ports`        | Allows listing of available serial ports                                      |
| `serialplugin:deny-available-ports`         | Denies listing of available serial ports                                      |
| `serialplugin:allow-cancel-read`            | Allows canceling of read operations                                           |
| `serialplugin:deny-cancel-read`             | Denies canceling of read operations                                           |
| `serialplugin:allow-close`                  | Allows closing of serial ports                                                |
| `serialplugin:deny-close`                   | Denies closing of serial ports                                                |
| `serialplugin:allow-close-all`              | Allows closing of all open serial ports                                       |
| `serialplugin:deny-close-all`               | Denies closing of all open serial ports                                       |
| `serialplugin:allow-force-close`            | Allows forcefully closing of serial ports                                     |
| `serialplugin:deny-force-close`             | Denies forcefully closing of serial ports                                     |
| `serialplugin:allow-open`                   | Allows opening of serial ports                                                |
| `serialplugin:deny-open`                    | Denies opening of serial ports                                                |
| `serialplugin:allow-read`                   | Allows reading data from serial ports                                         |
| `serialplugin:deny-read`                    | Denies reading data from serial ports                                         |
| `serialplugin:allow-read-binary`            | Allows reading binary data from serial ports                                  |
| `serialplugin:deny-read-binary`             | Denies reading binary data from serial ports                                  |
| `serialplugin:allow-write`                  | Allows writing data to serial ports                                           |
| `serialplugin:deny-write`                   | Denies writing data to serial ports                                           |
| `serialplugin:allow-write-binary`           | Allows writing binary data to serial ports                                    |
| `serialplugin:deny-write-binary`            | Denies writing binary data to serial ports                                    |
| `serialplugin:allow-available-ports-direct` | Enables the `available_ports_direct` command without any pre-configured scope |
| `serialplugin:deny-available-ports-direct`  | Denies the `available_ports_direct` command without any pre-configured scope  |
| `serialplugin:allow-set-baud-rate`          | Allows changing the baud rate of serial ports                                 |
| `serialplugin:deny-set-baud-rate`           | Denies changing the baud rate of serial ports                                 |
| `serialplugin:allow-set-data-bits`          | Allows changing the data bits configuration                                   |
| `serialplugin:deny-set-data-bits`           | Denies changing the data bits configuration                                   |
| `serialplugin:allow-set-flow-control`       | Allows changing the flow control mode                                         |
| `serialplugin:deny-set-flow-control`        | Denies changing the flow control mode                                         |
| `serialplugin:allow-set-parity`             | Allows changing the parity checking mode                                      |
| `serialplugin:deny-set-parity`              | Denies changing the parity checking mode                                      |
| `serialplugin:allow-set-stop-bits`          | Allows changing the stop bits configuration                                   |
| `serialplugin:deny-set-stop-bits`           | Denies changing the stop bits configuration                                   |
| `serialplugin:allow-set-timeout`            | Allows changing the timeout duration                                          |
| `serialplugin:deny-set-timeout`             | Denies changing the timeout duration                                          |
| `serialplugin:allow-write-rts`              | Allows setting the RTS (Request To Send) control signal                       |
| `serialplugin:deny-write-rts`               | Denies setting the RTS control signal                                         |
| `serialplugin:allow-write-dtr`              | Allows setting the DTR (Data Terminal Ready) control signal                   |
| `serialplugin:deny-write-dtr`               | Denies setting the DTR control signal                                         |
| `serialplugin:allow-read-cts`               | Allows reading the CTS (Clear To Send) control signal state                   |
| `serialplugin:deny-read-cts`                | Denies reading the CTS control signal state                                   |
| `serialplugin:allow-read-dsr`               | Allows reading the DSR (Data Set Ready) control signal state                  |
| `serialplugin:deny-read-dsr`                | Denies reading the DSR control signal state                                   |
| `serialplugin:allow-read-ri`                | Allows reading the RI (Ring Indicator) control signal state                   |
| `serialplugin:deny-read-ri`                 | Denies reading the RI control signal state                                    |
| `serialplugin:allow-read-cd`                | Allows reading the CD (Carrier Detect) control signal state                   |
| `serialplugin:deny-read-cd`                 | Denies reading the CD control signal state                                    |
| `serialplugin:allow-bytes-to-read`          | Allows checking the number of bytes available to read                         |
| `serialplugin:deny-bytes-to-read`           | Denies checking the number of bytes available to read                         |
| `serialplugin:allow-bytes-to-write`         | Allows checking the number of bytes waiting to be written                     |
| `serialplugin:deny-bytes-to-write`          | Denies checking the number of bytes waiting to be written                     |
| `serialplugin:allow-clear-buffer`           | Allows clearing input/output buffers                                          |
| `serialplugin:deny-clear-buffer`            | Denies clearing input/output buffers                                          |
| `serialplugin:allow-set-break`              | Allows starting break signal transmission                                     |
| `serialplugin:deny-set-break`               | Denies starting break signal transmission                                     |
| `serialplugin:allow-clear-break`            | Allows stopping break signal transmission                                     |
| `serialplugin:deny-clear-break`             | Denies stopping break signal transmission                                     |
| `serialplugin:allow-start-listening`        | Allows starting automatic port monitoring and data listening                  |
| `serialplugin:deny-start-listening`         | Denies starting automatic port monitoring and data listening                  |
| `serialplugin:allow-stop-listening`         | Allows stopping automatic port monitoring and data listening                  |
| `serialplugin:deny-stop-listening`          | Denies stopping automatic port monitoring and data listening                  |

### Granting All Permissions (Example)

```jsonc
"permissions": [
  "core:default",
  "serialplugin:default",
  "serialplugin:allow-available-ports",
  "serialplugin:allow-cancel-read",
  "serialplugin:allow-close",
  "serialplugin:allow-close-all",
  "serialplugin:allow-force-close",
  "serialplugin:allow-open",
  "serialplugin:allow-read",
  "serialplugin:allow-write",
  "serialplugin:allow-write-binary",
  "serialplugin:allow-available-ports-direct",
  "serialplugin:allow-set-baud-rate",
  "serialplugin:allow-set-data-bits",
  "serialplugin:allow-set-flow-control",
  "serialplugin:allow-set-parity",
  "serialplugin:allow-set-stop-bits",
  "serialplugin:allow-set-timeout",
  "serialplugin:allow-write-rts",
  "serialplugin:allow-write-dtr",
  "serialplugin:allow-read-cts",
  "serialplugin:allow-read-dsr",
  "serialplugin:allow-read-ri",
  "serialplugin:allow-read-cd",
  "serialplugin:allow-bytes-to-read",
  "serialplugin:allow-bytes-to-write",
  "serialplugin:allow-clear-buffer",
  "serialplugin:allow-set-break",
  "serialplugin:allow-clear-break",
  "serialplugin:allow-start-listening",
  "serialplugin:allow-stop-listening"
]
```

---

## API Reference

### Port Discovery

```typescript
class SerialPort {
  /**
   * Lists all available serial ports on the system
   * @returns {Promise<{[key: string]: PortInfo}>} Map of port names to port information
   * @example
   * const ports = await SerialPort.available_ports();
   * console.log(ports);
   */
  static async available_ports(): Promise<{ [key: string]: PortInfo }>;

  /**
   * Lists ports using platform-specific commands for enhanced detection
   * @returns {Promise<{[key: string]: PortInfo}>} Map of port names to port information
   * @example
   * const ports = await SerialPort.available_ports_direct();
   */
  static async available_ports_direct(): Promise<{ [key: string]: PortInfo }>;

  /**
   * @description Lists all managed serial ports (ports that are currently open and managed by the application).
   * @returns {Promise<string[]>} A promise that resolves to an array of port paths (names).
   */
  static async managed_ports(): Promise<string[]>;
}
```

### Connection Management

```typescript
class SerialPort {
  /**
   * Opens the serial port with specified configuration
   * @returns {Promise<void>}
   * @throws {Error} If port is already open or invalid configuration
   * @example
   * const port = new SerialPort({ path: "COM1", baudRate: 9600 });
   * await port.open();
   */
  async open(): Promise<void>;

  /**
   * Closes the serial port connection
   * @returns {Promise<void>}
   * @throws {Error} If port is not open
   * @example
   * await port.close();
   */
  async close(): Promise<void>;

  /**
   * Starts listening for data on the serial port
   * @returns {Promise<void>} A promise that resolves when listening starts
   * @throws {Error} If starting listener fails or port is not open
   * @example
   * await port.startListening();
   *
   * // Listen for data events
   * port.listen((data) => {
   *   console.log("Data received:", data);
   * });
   */
  async startListening(): Promise<void>;

  /**
   * Stops listening for data on the serial port
   * @returns {Promise<void>} A promise that resolves when listening stops
   * @throws {Error} If stopping listener fails or port is not open
   * @example
   * await port.stopListening();
   */
  async stopListening(): Promise<void>;

  /**
   * Forces a serial port to close regardless of its state
   * @param {string} path Port path to force close
   * @returns {Promise<void>}
   * @example
   * await SerialPort.forceClose("COM1");
   */
  static async forceClose(path: string): Promise<void>;

  /**
   * Closes all open serial port connections
   * @returns {Promise<void>}
   * @example
   * await SerialPort.closeAll();
   */
  static async closeAll(): Promise<void>;
}
```

### Data Transfer

```typescript
class SerialPort {
  /**
   * Writes string data to the serial port
   * @param {string} data Data to write
   * @returns {Promise<number>} Number of bytes written
   * @throws {Error} If write fails or port is not open
   * @example
   * const bytesWritten = await port.write("Hello");
   */
  async write(data: string): Promise<number>;

  /**
   * Reads data from the serial port
   * @param {ReadOptions} [options] Read options
   * @returns {Promise<string>} A promise that resolves to a string
   */
  async read(options?: ReadOptions): Promise<string>;

  /**
   * Reads binary data from the serial port
   * @param {ReadOptions} [options] Read options
   * @returns {Promise<Uint8Array>} A promise that resolves with binary data
   */
  async readBinary(options?: ReadOptions): Promise<Uint8Array>;

  /**
   * Writes binary data to the serial port
   * @param {Uint8Array | number[]} data Binary data to write
   * @returns {Promise<number>} Number of bytes written
   * @throws {Error} If write fails or port is not open
   * @example
   * const data = new Uint8Array([0x01, 0x02, 0x03]);
   * const bytesWritten = await port.writeBinary(data);
   */
  async writeBinary(data: Uint8Array | number[]): Promise<number>;

  /**
   * Sets up a listener for incoming data
   * @param {(data: string | Uint8Array) => void} callback Function to handle received data
   * @param {boolean} [decode=true] Whether to decode data as string (true) or return raw bytes (false)
   * @returns {Promise<UnlistenFn>} A promise that resolves to an unlisten function
   * @example
   * const unsubscribe = await port.listen((data) => {
   *   console.log("Received:", data);
   * });
   * 
   * // Later, to stop listening:
   * unsubscribe();
   */
  async listen(callback: (data: string | Uint8Array) => void, decode?: boolean): Promise<UnlistenFn>;

  /**
   * Cancels listening for serial port data (does not affect disconnect listeners)
   * @returns {Promise<void>} A promise that resolves when listening is cancelled
   * @example
   * await port.cancelListen();
   */
  async cancelListen(): Promise<void>;
}
```

### Port Configuration

```typescript
class SerialPort {
  /**
   * Sets the baud rate
   * @param {number} baudRate Speed in bits per second
   * @returns {Promise<void>}
   * @example
   * await port.setBaudRate(115200);
   */
  async setBaudRate(baudRate: number): Promise<void>;

  /**
   * Sets the number of data bits
   * @param {DataBits} dataBits Number of bits per character (5-8)
   * @returns {Promise<void>}
   * @example
   * await port.setDataBits(DataBits.Eight);
   */
  async setDataBits(dataBits: DataBits): Promise<void>;

  /**
   * Sets the flow control mode
   * @param {FlowControl} flowControl Flow control setting
   * @returns {Promise<void>}
   * @example
   * await port.setFlowControl(FlowControl.Hardware);
   */
  async setFlowControl(flowControl: FlowControl): Promise<void>;

  /**
   * Sets the parity checking mode
   * @param {Parity} parity Parity checking mode
   * @returns {Promise<void>}
   * @example
   * await port.setParity(Parity.None);
   */
  async setParity(parity: Parity): Promise<void>;

  /**
   * Sets the number of stop bits
   * @param {StopBits} stopBits Number of stop bits
   * @returns {Promise<void>}
   * @example
   * await port.setStopBits(StopBits.One);
   */
  async setStopBits(stopBits: StopBits): Promise<void>;

  /**
   * Sets the timeout for read operations
   * @param {number} timeout Timeout value in milliseconds
   * @returns {Promise<void>}
   * @example
   * await port.setTimeout(1000);
   */
  async setTimeout(timeout: number): Promise<void>;
}
```

### Control Signals

```typescript
class SerialPort {
  /**
   * Sets the RTS (Request to Send) signal
   * @param {boolean} level Signal level (true = high, false = low)
   * @returns {Promise<void>}
   * @example
   * await port.writeRequestToSend(true);
   */
  async writeRequestToSend(level: boolean): Promise<void>;

  /**
   * Sets the DTR (Data Terminal Ready) signal
   * @param {boolean} level Signal level (true = high, false = low)
   * @returns {Promise<void>}
   * @example
   * await port.writeDataTerminalReady(true);
   */
  async writeDataTerminalReady(level: boolean): Promise<void>;

  /**
   * Alternative method to set RTS signal
   * @param {boolean} value Signal level (true = high, false = low)
   * @returns {Promise<void>}
   * @example
   * await port.setRequestToSend(true);
   */
  async setRequestToSend(value: boolean): Promise<void>;

  /**
   * Alternative method to set DTR signal
   * @param {boolean} value Signal level (true = high, false = low)
   * @returns {Promise<void>}
   * @example
   * await port.setDataTerminalReady(true);
   */
  async setDataTerminalReady(value: boolean): Promise<void>;

  /**
   * Reads the CTS (Clear to Send) signal state
   * @returns {Promise<boolean>} Signal state
   * @example
   * const cts = await port.readClearToSend();
   */
  async readClearToSend(): Promise<boolean>;

  /**
   * Reads the DSR (Data Set Ready) signal state
   * @returns {Promise<boolean>} Signal state
   * @example
   * const dsr = await port.readDataSetReady();
   */
  async readDataSetReady(): Promise<boolean>;

  /**
   * Reads the RI (Ring Indicator) signal state
   * @returns {Promise<boolean>} Signal state
   * @example
   * const ri = await port.readRingIndicator();
   */
  async readRingIndicator(): Promise<boolean>;

  /**
   * Reads the CD (Carrier Detect) signal state
   * @returns {Promise<boolean>} Signal state
   * @example
   * const cd = await port.readCarrierDetect();
   */
  async readCarrierDetect(): Promise<boolean>;
}
```

### Buffer Management

```typescript
class SerialPort {
  /**
   * Gets number of bytes available to read
   * @returns {Promise<number>} Number of bytes in read buffer
   * @example
   * const available = await port.bytesToRead();
   */
  async bytesToRead(): Promise<number>;

  /**
   * Gets number of bytes waiting to be written
   * @returns {Promise<number>} Number of bytes in write buffer
   * @example
   * const pending = await port.bytesToWrite();
   */
  async bytesToWrite(): Promise<number>;

  /**
   * Clears the specified buffer
   * @param {ClearBuffer} buffer Buffer to clear
   * @returns {Promise<void>}
   * @example
   * await port.clearBuffer(ClearBuffer.Input);
   */
  async clearBuffer(buffer: ClearBuffer): Promise<void>;

  /**
   * Sets the break signal
   * @returns {Promise<void>}
   * @example
   * await port.setBreak();
   */
  async setBreak(): Promise<void>;

  /**
   * Clears the break signal
   * @returns {Promise<void>}
   * @example
   * await port.clearBreak();
   */
  async clearBreak(): Promise<void>;
}
```

### Auto-Reconnect Management

```typescript
class SerialPort {
  /**
   * Enables auto-reconnect functionality
   * @param {Object} options Auto-reconnect configuration options
   * @param {number} [options.interval=5000] Reconnection interval in milliseconds
   * @param {number | null} [options.maxAttempts=10] Maximum number of reconnection attempts (null for infinite)
   * @param {Function} [options.onReconnect] Callback function called on each reconnection attempt
   * @returns {Promise<void>}
   * @example
   * await port.enableAutoReconnect({
   *   interval: 3000,
   *   maxAttempts: 5,
   *   onReconnect: (success, attempt) => {
   *     console.log(`Reconnect attempt ${attempt}: ${success ? 'success' : 'failed'}`);
   *   }
   * });
   */
  async enableAutoReconnect(options?: {
    interval?: number;
    maxAttempts?: number | null;
    onReconnect?: (success: boolean, attempt: number) => void;
  }): Promise<void>;

  /**
   * Disables auto-reconnect functionality
   * @returns {Promise<void>}
   * @example
   * await port.disableAutoReconnect();
   */
  async disableAutoReconnect(): Promise<void>;

  /**
   * Gets auto-reconnect status and configuration
   * @returns {Object} Auto-reconnect information
   * @example
   * const info = port.getAutoReconnectInfo();
   * console.log('Auto-reconnect enabled:', info.enabled);
   * console.log('Current attempts:', info.currentAttempts);
   */
  getAutoReconnectInfo(): {
    enabled: boolean;
    interval: number;
    maxAttempts: number | null;
    currentAttempts: number;
    hasCallback: boolean;
  };

  /**
   * Manually triggers a reconnection attempt
   * @returns {Promise<boolean>} A promise that resolves to true if reconnection was successful
   * @example
   * const success = await port.manualReconnect();
   * if (success) {
   *   console.log('Manual reconnection successful');
   * }
   */
  async manualReconnect(): Promise<boolean>;
}
```

## Common Use Cases

### Reading Sensor Data

```typescript
const port = new SerialPort({
  path: "COM1",
  baudRate: 9600
});

await port.open();
await port.listen((data) => {
  const sensorValue = parseFloat(data);
  console.log("Sensor reading:", sensorValue);
});
```

### Binary Protocol Communication

```typescript
const port = new SerialPort({
  path: "COM1",
  baudRate: 115200
});

await port.open();

// Send command
const command = new Uint8Array([0x02, 0x01, 0x03]);
await port.writeBinary(command);

// Read response (raw bytes)
await port.listen((data) => {
  const response = data instanceof Uint8Array ? data : new Uint8Array();
  console.log("Response:", response);
}, false);
```

### Modbus Communication

```typescript
const port = new SerialPort({
   path: "COM1",
   baudRate: 9600,
   dataBits: DataBits.Eight,
   stopBits: StopBits.One,
   parity: Parity.None
});

await port.open();

function createModbusRequest(address: number, length: number): Uint8Array {
   return new Uint8Array([
      0x01, // Device ID
      0x03, // Function code: Read Holding Registers
      address >> 8, address & 0xFF,
      length >> 8, length & 0xFF
   ]);
}

// Send Modbus request
const request = createModbusRequest(0x1000, 10);
await port.writeBinary(request);
```

### Auto-Reconnect for Reliable Communication

```typescript
const port = new SerialPort({
  path: "COM1",
  baudRate: 9600
});

await port.open();

// Enable auto-reconnect with custom settings
await port.enableAutoReconnect({
  interval: 3000,        // Try to reconnect every 3 seconds
  maxAttempts: 5,        // Maximum 5 attempts
  onReconnect: (success, attempt) => {
    if (success) {
      console.log(`Reconnected successfully on attempt ${attempt}`);
    } else {
      console.log(`Reconnection attempt ${attempt} failed`);
    }
  }
});

// Set up data listener
const unsubscribe = await port.listen((data) => {
  console.log("Received data:", data);
});

// The port will automatically reconnect if disconnected
// You can also manually trigger reconnection
const success = await port.manualReconnect();
if (success) {
  console.log("Manual reconnection successful");
}

// Check auto-reconnect status
const info = port.getAutoReconnectInfo();
console.log("Auto-reconnect enabled:", info.enabled);
console.log("Current attempts:", info.currentAttempts);

// Disable auto-reconnect when no longer needed
await port.disableAutoReconnect();
```

---

## Android Setup

To use this plugin on Android, you need to add the JitPack repository to your project's `build.gradle.kts` file located at `/src-tauri/gen/android/build.gradle.kts`. Below is an example of how to configure it:

```kotlin
buildscript {
    repositories {
        // ...
        maven { url = uri("https://jitpack.io") }
    }
    // ...
}

allprojects {
    repositories {
        // ...
        maven { url = uri("https://jitpack.io") }
    }
}
```

---

## Contributing

Pull requests are welcome! Please read our contributing guidelines before you start.

---

## Development Setup

```bash
git clone https://github.com/s00d/tauri-plugin-serialplugin.git
cd tauri-plugin-serialplugin

pnpm i
pnpm run build
pnpm run playground
```

## Testing

For testing applications without physical hardware, you can use a mock implementation of the serial port. The mock port emulates all functions of a real port and allows testing the application without physical devices.

### Using Mock Port

```rust
use tauri_plugin_serialplugin::tests::mock::MockSerialPort;

// Create a mock port
let mock_port = MockSerialPort::new();

// Configure port settings
mock_port.set_baud_rate(9600).unwrap();
mock_port.set_data_bits(serialport::DataBits::Eight).unwrap();
mock_port.set_flow_control(serialport::FlowControl::None).unwrap();
mock_port.set_parity(serialport::Parity::None).unwrap();
mock_port.set_stop_bits(serialport::StopBits::One).unwrap();

// Write data
mock_port.write("Test data".as_bytes()).unwrap();

// Read data
let mut buffer = [0u8; 1024];
let bytes_read = mock_port.read(&mut buffer).unwrap();
let data = String::from_utf8_lossy(&buffer[..bytes_read]);
assert_eq!(data, "Test data");
```

### Mock Port Features

- Complete emulation of all real port functions
- Built-in buffer for data storage
- Control signal emulation (RTS, DTR, CTS, DSR)
- Support for parallel operation testing
- No additional software required
- Works on all platforms

### Application Testing Example

```rust
#[test]
fn test_serial_communication() {
    let app = create_test_app();
    let serial_port = SerialPort::new(app.handle().clone());
    app.manage(serial_port);

    // Open mock port
    app.state::<SerialPort<MockRuntime>>().open(
        "COM1".to_string(),
        9600,
        Some(DataBits::Eight),
        Some(FlowControl::None),
        Some(Parity::None),
        Some(StopBits::One),
        Some(1000),
    ).unwrap();

    // Test write and read operations
    app.state::<SerialPort<MockRuntime>>().write(
        "COM1".to_string(),
        "Test data".to_string(),
    ).unwrap();

    let data = app.state::<SerialPort<MockRuntime>>().read(
        "COM1".to_string(),
        Some(1000),
        Some(1024),
    ).unwrap();
    assert_eq!(data, "Test data");

    // Test port settings
    app.state::<SerialPort<MockRuntime>>().set_baud_rate(
        "COM1".to_string(),
        115200,
    ).unwrap();

    // Close port
    app.state::<SerialPort<MockRuntime>>().close("COM1".to_string()).unwrap();
}
```

### Implementing Your Own Mock Port

You can implement your own mock port by implementing the `SerialPort` trait. Here's a basic example of how to create a custom mock port:

```rust
use std::io::{self, Read, Write};
use serialport::{self, SerialPort};
use std::time::Duration;

struct CustomMockPort {
    buffer: Vec<u8>,
    baud_rate: u32,
    data_bits: serialport::DataBits,
    flow_control: serialport::FlowControl,
    parity: serialport::Parity,
    stop_bits: serialport::StopBits,
    timeout: Duration,
}

impl CustomMockPort {
    fn new() -> Self {
        Self {
            buffer: Vec::new(),
            baud_rate: 9600,
            data_bits: serialport::DataBits::Eight,
            flow_control: serialport::FlowControl::None,
            parity: serialport::Parity::None,
            stop_bits: serialport::StopBits::One,
            timeout: Duration::from_millis(1000),
        }
    }
}

// Implement Read trait for reading data
impl Read for CustomMockPort {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let len = std::cmp::min(buf.len(), self.buffer.len());
        if len > 0 {
            buf[..len].copy_from_slice(&self.buffer[..len]);
            self.buffer.drain(..len);
        }
        Ok(len)
    }
}

// Implement Write trait for writing data
impl Write for CustomMockPort {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.buffer.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

// Implement SerialPort trait for port configuration
impl SerialPort for CustomMockPort {
    fn name(&self) -> Option<String> {
        Some("CUSTOM_PORT".to_string())
    }

    fn baud_rate(&self) -> serialport::Result<u32> {
        Ok(self.baud_rate)
    }

    fn data_bits(&self) -> serialport::Result<serialport::DataBits> {
        Ok(self.data_bits)
    }

    // ... implement other required methods ...
}
```

For a complete implementation example, see the mock port implementation in the plugin's test directory:
[`src/tests/mock.rs`](https://github.com/s00d/tauri-plugin-serialplugin/blob/main/src/tests/mock.rs)

The example includes:
- Full implementation of all required traits
- Buffer management for read/write operations
- Control signal emulation
- Port configuration handling
- Error handling
- Thread safety considerations

You can use this implementation as a reference when creating your own mock port with custom behavior for specific testing scenarios.

---

## Partners

If you find this plugin valuable and would like to support further development, feel free to donate via [DonationAlerts](https://www.donationalerts.com/r/s00d88). Any contribution is greatly appreciated!

---

## License

This code is dual-licensed under MIT or Apache-2.0, where applicable, © 2019–2025 Tauri Programme within The Commons Conservancy.
