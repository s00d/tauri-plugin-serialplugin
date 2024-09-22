# Tauri Plugin - SerialPort

This plugin enables Tauri applications to communicate with serial ports, allowing for the reading and writing of data to and from connected serial devices.

## Installation

_This plugin requires Rust version **1.70** or higher._

There are three recommended methods for installing this plugin:

1. **Using crates.io and npm** (easiest, requires trust in our publishing pipeline)
2. **Directly from GitHub using git tags/revision hashes** (most secure)
3. **Git submodule in your Tauri project, then using the file protocol for source inclusion** (most secure but less convenient)

### Core Plugin

Add the following to your `Cargo.toml` file under `src-tauri/Cargo.toml`:

```toml
[dependencies]
tauri-plugin-serialplugin = "2.0.0-rc.3"
```

### JavaScript Bindings

Install using your preferred package manager:

```sh
pnpm add tauri-plugin-serialplugin
# or
npm add tauri-plugin-serialplugin
# or
yarn add tauri-plugin-serialplugin

# For direct GitHub installation:
pnpm add https://github.com/s00d/tauri-plugin-serialplugin#main
# or
npm add https://github.com/s00d/tauri-plugin-serialplugin#main
# or
yarn add https://github.com/s00d/tauri-plugin-serialplugin#main
```

## Usage

First, register the core plugin within your Tauri application's main setup:

`src-tauri/src/main.rs`

```rust
fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_serialplugin::init())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

### Configuring Permissions

To use the plugin, you must define permissions in your capabilities configuration. Add the following to your `/src-tauri/capabilities/default.json` file:

```json
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

The `serialplugin:default` permission allows basic usage of the plugin. If you need more granular permissions, you can specify them as follows:

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "Capability for the main window",
  "windows": ["main"],
  "permissions": [
    "core:default",
    "serialplugin:allow-available-ports",
    "serialplugin:allow-available-ports-direct",
    "serialplugin:allow-cancel-read",
    "serialplugin:allow-close",
    "serialplugin:allow-close-all",
    "serialplugin:allow-force-close",
    "serialplugin:allow-open",
    "serialplugin:allow-read",
    "serialplugin:allow-write",
    "serialplugin:allow-write-binary"
  ]
}
```

### Permission Descriptions

| Permission                                  | Description                                                                    |
|---------------------------------------------|--------------------------------------------------------------------------------|
| `serialplugin:allow-available-ports`        | Allows listing of available serial ports                                       |
| `serialplugin:deny-available-ports`         | Denies listing of available serial ports                                       |
| `serialplugin:allow-cancel-read`            | Allows canceling of read operations                                            |
| `serialplugin:deny-cancel-read`             | Denies canceling of read operations                                            |
| `serialplugin:allow-close`                  | Allows closing of serial ports                                                 |
| `serialplugin:deny-close`                   | Denies closing of serial ports                                                 |
| `serialplugin:allow-close-all`              | Allows closing of all open serial ports                                        |
| `serialplugin:deny-close-all`               | Denies closing of all open serial ports                                        |
| `serialplugin:allow-force-close`            | Allows forcefully closing of serial ports                                      |
| `serialplugin:deny-force-close`             | Denies forcefully closing of serial ports                                      |
| `serialplugin:allow-open`                   | Allows opening of serial ports                                                 |
| `serialplugin:deny-open`                    | Denies opening of serial ports                                                 |
| `serialplugin:allow-read`                   | Allows reading data from serial ports                                          |
| `serialplugin:deny-read`                    | Denies reading data from serial ports                                          |
| `serialplugin:allow-write`                  | Allows writing data to serial ports                                            |
| `serialplugin:deny-write`                   | Denies writing data to serial ports                                            |
| `serialplugin:allow-write-binary`           | Allows writing binary data to serial ports                                     |
| `serialplugin:deny-write-binary`            | Denies writing binary data to serial ports                                     |
| `serialplugin:allow-available-ports-direct` | Enables the `available_ports_direct` command without any pre-configured scope. |
| `serialplugin:deny-available-ports-direct`  | Denies the `available_ports_direct` command without any pre-configured scope.  |

## Example Application

An example application can be found in the `examples/serialport-test` directory of this repository. This example demonstrates basic usage of the plugin, including opening, closing, and listing serial ports.

### JavaScript API

After registering the plugin, you can access the plugin's APIs through the provided JavaScript bindings:

```javascript
import { SerialPort } from "tauri-plugin-serialplugin";

// Example: Listing available serial ports
async function listPorts() {
  const ports = await SerialPort.available_ports();
  console.log(ports);
}

listPorts();
```

### Additional Methods

#### `SerialPort.available_ports()`

Lists all available serial ports.

#### `SerialPort.forceClose(path: string)`

Forcefully closes the specified serial port.

#### `SerialPort.available_ports_direct()`

Retrieves a list of available serial ports using platform-specific commands. This method serves as a fallback in case `available_ports` fails to return results. It checks for connected serial ports on Windows, Linux, and macOS, returning their names in a `HashMap` format where the key is the port name and the value contains additional information about the port.

#### `SerialPort.closeAll()`

Closes all open serial ports.

#### `SerialPort.cancelListen()`

Cancels serial port monitoring.

#### `SerialPort.cancelRead()`

Cancels reading data from the serial port.

#### `SerialPort.change(options: { path?: string; baudRate?: number })`

Changes the path and/or baud rate of the serial port.

#### `SerialPort.close()`

Closes the currently opened serial port.

#### `SerialPort.disconnected(fn: (...args: any[]) => void)`

Sets up a listener for when the serial port is disconnected.

#### `SerialPort.listen(fn: (...args: any[]) => void, isDecode = true)`

Monitors serial port information and handles data using the provided callback function.

#### `SerialPort.open()`

Opens the serial port with the specified settings.

#### `SerialPort.read(options?: ReadOptions)`

Reads data from the serial port with optional settings for timeout and size.

#### `SerialPort.setBaudRate(value: number)`

Sets the baud rate of the serial port.

#### `SerialPort.setPath(value: string)`

Sets the path of the serial port.

#### `SerialPort.write(value: string)`

Writes data to the serial port.

#### `SerialPort.writeBinary(value: Uint8Array | number[])`

Writes binary data to the serial port.

### Example Code

Below is a small example demonstrating how to use the plugin to open, close, and list available serial ports:

```typescript
import { SerialPort } from 'tauri-plugin-serialplugin';

let serialport: SerialPort | undefined = undefined;
let name: string;

function openPort() {
  serialport = new SerialPort({ path: name, baudRate: 9600 });
  serialport
    .open()
    .then((res) => {
      console.log('open serialport', res);
    })
    .catch((err) => {
      console.error(err);
    });
}

function closePort() {
  serialport
    .close()
    .then((res) => {
      console.log('close serialport', res);
    })
    .catch((err) => {
      console.error(err);
    });
}

function availablePorts() {
  SerialPort.available_ports()
    .then((res) => {
      console.log('available_ports: ', res);
    })
    .catch((err) => {
      console.error(err);
    });
}
```

## Contributing

We welcome pull requests! Please ensure you read our Contributing Guide before submitting a pull request.

## Partners

Support for this plugin is provided by our generous partners. For a complete list, please visit our [website](https://tauri.app#sponsors) and our [Open Collective](https://opencollective.com/tauri).

## License

This code is dual-licensed under MIT or Apache-2.0, where applicable, © 2019-2023 Tauri Programme within The Commons Conservancy.

---

This README provides an overview, installation instructions, and basic usage examples for integrating serial port communication into a Tauri application. Further details and advanced usage should be documented based on the full capabilities of the plugin and its API.
