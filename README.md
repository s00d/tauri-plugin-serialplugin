Based on the provided code snippet and following the structure of the example you shared, here's a README for the Tauri plugin for serial port communication.

---

# Tauri Plugin - SerialPort

This plugin enables Tauri applications to communicate with serial ports, allowing for the reading and writing of data to and from connected serial devices. To manage child processes instead, consider using the [`shell`](https://github.com/tauri-apps/tauri-plugin-shell) plugin.

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
tauri-plugin-serialport = "2.0.0-beta"
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
pnpm add https://github.com/s00d/tauri-plugin-serialplugin#v2
# or
npm add https://github.com/s00d/tauri-plugin-serialplugin#v2
# or
yarn add https://github.com/s00d/tauri-plugin-serialport#v2
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

After registration, you can access the plugin's APIs through the provided JavaScript bindings:

```javascript
import { SerialPort } from "tauri-plugin-serialplugin";

// Example: Listing available serial ports
async function listPorts() {
  const ports = await SerialPort.available_ports();
  console.log(ports);
}

listPorts();
```

## Contributing

We welcome pull requests! Please ensure you read our Contributing Guide before submitting a pull request.

## Partners

Support for this plugin is provided by our generous partners. For a complete list, please visit our [website](https://tauri.app#sponsors) and our [Open Collective](https://opencollective.com/tauri).

## License

This code is dual-licensed under MIT or Apache-2.0, where applicable, © 2019-2023 Tauri Programme within The Commons Conservancy.

---

This README provides an overview, installation instructions, and basic usage examples for integrating serial port communication into a Tauri application. Further details and advanced usage should be documented based on the full capabilities of the plugin and its API.