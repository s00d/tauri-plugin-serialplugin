[![npm version](https://img.shields.io/npm/v/tauri-plugin-serialplugin/latest?style=for-the-badge)](https://www.npmjs.com/package/tauri-plugin-serialplugin)
[![Crates.io](https://img.shields.io/crates/v/tauri-plugin-serialplugin?style=for-the-badge)](https://crates.io/crates/tauri-plugin-serialplugin)
[![GitHub issues](https://img.shields.io/github/issues/s00d/tauri-plugin-serialplugin?style=for-the-badge)](https://github.com/s00d/tauri-plugin-serialplugin/issues)
[![GitHub stars](https://img.shields.io/github/stars/s00d/tauri-plugin-serialplugin?style=for-the-badge)](https://github.com/s00d/tauri-plugin-serialplugin/stargazers)
[![Donate](https://img.shields.io/badge/Donate-Donationalerts-ff4081?style=for-the-badge)](https://www.donationalerts.com/r/s00d88)

# Tauri Plugin — SerialPort

A comprehensive plugin for Tauri applications to communicate with serial ports. This plugin provides a complete API for reading from and writing to serial devices, with support for various configuration options and control signals.

---

## Table of Contents

1. [Installation](#installation)
2. [Basic Usage](#basic-usage)
3. [Permissions](#permissions)
4. [API Reference](#api-reference)  
   4.1. [Port Discovery](#port-discovery)  
   4.2. [Connection Management](#connection-management)  
   4.3. [Data Transfer](#data-transfer)  
   4.4. [Port Configuration](#port-configuration)  
   4.5. [Control Signals](#control-signals)  
   4.6. [Buffer Management](#buffer-management)
5. [Common Use Cases](#common-use-cases)
6. [Android Setup](#android-setup)
7. [Contributing](#contributing)
8. [Development Setup](#development-setup)
9. [Testing](#testing)
10. [Partners](#partners)
11. [License](#license)

---

## Installation

### Prerequisites

- **Rust** version 1.70 or higher
- **Tauri** 2.0 or higher
- **Node.js** and an npm-compatible package manager (npm, yarn, pnpm)

### Installation Methods

**Using crates.io and npm (Recommended)**

```bash
# Install the Rust dependency
cargo add tauri-plugin-serialplugin
```

```bash
# Install JavaScript bindings
npm add tauri-plugin-serialplugin
# or
yarn add tauri-plugin-serialplugin
# or
pnpm add tauri-plugin-serialplugin
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
   import { SerialPort } from "tauri-plugin-serialplugin";

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
   await port.listen((data) => {
     console.log("Received:", data);
   });

   // Stop listening when done
   await port.cancelListen();

   // Close port
   await port.close();
   ```

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
   * @returns {Promise<void>}
   * @example
   * await port.listen((data) => {
   *   console.log("Received:", data);
   * });
   */
  async listen(callback: (data: string | Uint8Array) => void, decode?: boolean): Promise<void>;
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
}
```

## Error Types

The plugin uses a comprehensive error handling system that covers various scenarios that may occur during serial port operations. Here are the different types of errors that can be returned:

### Error Types

| Error Type | Description |
|------------|-------------|
| `Io` | IO-related errors from the operating system (e.g., file system errors, device access issues) |
| `SerialPort` | Errors specific to serial port operations from the underlying serial port library |
| `InvalidPort` | Errors related to invalid port configuration or settings |
| `PortNotFound` | Errors when attempting to access a non-existent port |
| `PortAlreadyOpen` | Errors when trying to open a port that is already in use |
| `PortNotOpen` | Errors when attempting operations on a closed port |
| `PermissionDenied` | Errors due to insufficient permissions to access the port |
| `DeviceBusy` | Errors when the device is busy or locked by another process |
| `Timeout` | Errors that occur when operations exceed their timeout limits |
| `Custom` | Custom error messages for specific application scenarios |
| `MutexPoisoned` | Errors that occur when a mutex lock is poisoned during concurrent operations |

### Error Handling Example

```typescript
import { SerialPort, SerialErrorType, createSerialError } from "tauri-plugin-serialplugin";

try {
  const port = new SerialPort({
    path: "COM1",
    baudRate: 9600
  });
  
  await port.open();
} catch (error) {
  const serialError = createSerialError(error as string);
  
  switch (serialError.type) {
    case SerialErrorType.PortNotFound:
      console.error("Порт не найден:", serialError.message);
      break;
    case SerialErrorType.PortAlreadyOpen:
      console.error("Порт уже открыт:", serialError.message);
      break;
    case SerialErrorType.PermissionDenied:
      console.error("Отказано в доступе:", serialError.message);
      break;
    case SerialErrorType.DeviceBusy:
      console.error("Устройство занято:", serialError.message);
      break;
    case SerialErrorType.Timeout:
      console.error("Превышено время ожидания:", serialError.message);
      break;
    case SerialErrorType.Io:
      console.error("Ошибка ввода-вывода:", serialError.message);
      break;
    case SerialErrorType.SerialPort:
      console.error("Ошибка последовательного порта:", serialError.message);
      break;
    case SerialErrorType.MutexPoisoned:
      console.error("Ошибка блокировки:", serialError.message);
      break;
    default:
      console.error("Неизвестная ошибка:", serialError.message);
  }
}
```

---

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

The plugin includes tests for both Rust and JavaScript parts:

### Rust Tests
To run Rust tests, use the command:
```bash
cargo test
```

### JavaScript Tests
To run JavaScript tests, use the command:
```bash
pnpm jest
```

JavaScript tests are located in the `guest-js/` directory and use Jest as the testing framework. They cover all major plugin API functions, including:
- Port management
- Data reading and writing
- Port configuration
- Signal management
- Buffer operations

---

## Partners

If you find this plugin valuable and would like to support further development, feel free to donate via [DonationAlerts](https://www.donationalerts.com/r/s00d88). Any contribution is greatly appreciated!

---

## License

This code is dual-licensed under MIT or Apache-2.0, where applicable, © 2019–2025 Tauri Programme within The Commons Conservancy.