# Changelog

All notable changes to this project will be documented in this file. See [standard-version](https://github.com/conventional-changelog/standard-version) for commit guidelines.

## [2.15.0](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.14.0...v2.15.0) (2025-07-01)


### Features

* **listener-manager:** implement listener management for serial events ([b32b01c](https://github.com/s00d/tauri-plugin-serialplugin/commit/b32b01c063ddba33e9440acd1be5c4eddafdd9e9))

## [2.14.0](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.13.0...v2.14.0) (2025-07-01)


### Features

* **serial:** enhance error handling in serial port functions ([1403b77](https://github.com/s00d/tauri-plugin-serialplugin/commit/1403b777361eec135b1287434b9cee0452977948))

## [2.13.0](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.12.1...v2.13.0) (2025-07-01)


### Features

* **api-iife:** update event listener management in serial plugin ([81f4522](https://github.com/s00d/tauri-plugin-serialplugin/commit/81f452229b7a34e2b4d6e3cad28e40ce9780d021))

### [2.12.1](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.12.0...v2.12.1) (2025-06-19)


### Bug Fixes

* **deps:** update tauri-plugin-serialplugin to version 2.12.0 ([07e7526](https://github.com/s00d/tauri-plugin-serialplugin/commit/07e75261713db36fa9743c0230bd7dbe620a61b3))
* **mobile_api:** correct port opening error handling logic ([33f43c7](https://github.com/s00d/tauri-plugin-serialplugin/commit/33f43c7604c38baa6652662a16b28319d49563f7))

## [2.12.0](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.11.1...v2.12.0) (2025-06-12)


### Features

* **serialport:** add test mode for simulating serial port behavior ([22356b5](https://github.com/s00d/tauri-plugin-serialplugin/commit/22356b5cd229acb34d688226455673ec2ae96afd))
* **serialport:** add test mode for simulating serial port behavior ([287ab53](https://github.com/s00d/tauri-plugin-serialplugin/commit/287ab538db2022f27dae8c933f566d7ab052f77c))
* **serialport:** improve error handling and add port configuration ([5844e2d](https://github.com/s00d/tauri-plugin-serialplugin/commit/5844e2d2b715560f22dde9f499c316b338bf0410))
* **tests:** add comprehensive tests for serial port functionality ([614901e](https://github.com/s00d/tauri-plugin-serialplugin/commit/614901e6cdabcac22befba01debe47c8b599a94a))

### [2.11.1](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.11.0...v2.11.1) (2025-05-25)

## [2.11.0](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.10.2...v2.11.0) (2025-04-04)


### Features

* **serial:** set a default timeout for serial port connection ([c252883](https://github.com/s00d/tauri-plugin-serialplugin/commit/c2528839755ed1c6296b1eb5240e3e1548e3afc3))


### Bug Fixes

* **deps:** update dependencies to latest versions ([0d857bb](https://github.com/s00d/tauri-plugin-serialplugin/commit/0d857bbc5440081d3cf21683af2de2942fccbe9b))
* **deps:** update dependencies to latest versions ([c8dc8f1](https://github.com/s00d/tauri-plugin-serialplugin/commit/c8dc8f1e514c8b166c18895067e1fbc824831659))

### [2.10.2](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.10.1...v2.10.2) (2025-03-27)

### [2.10.1](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.10.0...v2.10.1) (2025-03-13)


### Bug Fixes

* **serialport:** update USB permission handling for Android 33+ ([b9b0e70](https://github.com/s00d/tauri-plugin-serialplugin/commit/b9b0e70de553b21ada8d1f66b33b7686f9d8ea8e))
* **serialport:** update USB permission handling for Android 33+ ([8034668](https://github.com/s00d/tauri-plugin-serialplugin/commit/80346681df190f98ed90fc8c17325cd111f31c45))

## [2.10.0](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.9.0...v2.10.0) (2025-03-02)


### Features

* **serialport-test:** refactor app structure and enhance port management ([22b0e7a](https://github.com/s00d/tauri-plugin-serialplugin/commit/22b0e7a93e0fefb91766c56ffe630024804f42a2))


### Bug Fixes

* **serialport:** update USB permission handling for Android 33+ ([e71cca9](https://github.com/s00d/tauri-plugin-serialplugin/commit/e71cca9508af4acea55deee432300263435a74d8))

## [2.9.0](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.8.2...v2.9.0) (2025-02-05)


### Features

* **android:** enable hardware acceleration in AndroidManifest.xml ([2f16b06](https://github.com/s00d/tauri-plugin-serialplugin/commit/2f16b066e4e8c53d5c1ba7b225d2c0b6eeb69fe0))
* **serialport:** register USB receiver in SerialPortManager ([5f32352](https://github.com/s00d/tauri-plugin-serialplugin/commit/5f323529b2481fcf0fe54f5264fcd0f0b61c95a9))


### Bug Fixes

* **device_filter:** remove outdated USB device entries ([3236183](https://github.com/s00d/tauri-plugin-serialplugin/commit/3236183c449758d30fabf339bba203721bbefc5b))

### [2.8.2](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.8.1...v2.8.2) (2025-02-02)


### Bug Fixes

* **serialport:** improve timeout handling and error messages ([de2233c](https://github.com/s00d/tauri-plugin-serialplugin/commit/de2233c0d2503b37e75401656e16842a54ca5a08))

### [2.8.1](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.8.0...v2.8.1) (2025-02-02)


### Bug Fixes

* **permissions:** add read-binary command permissions ([76262b1](https://github.com/s00d/tauri-plugin-serialplugin/commit/76262b10f3f75878a8d5f6226a60a27b06bdcdb8))

## [2.8.0](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.7.0...v2.8.0) (2025-01-30)


### Features

* **serial:** add `read_binary` command to read binary data from serial port ([1a17f99](https://github.com/s00d/tauri-plugin-serialplugin/commit/1a17f99dff08c430ba29bda0c0df8a883746e65e))
* **serial:** add `read_binary` command to read binary data from serial port ([83a873d](https://github.com/s00d/tauri-plugin-serialplugin/commit/83a873d6a02a3c146698bd651fa67dcb7dd75acd))
* **serial:** add `read_binary` command to read binary data from serial port ([09e6a32](https://github.com/s00d/tauri-plugin-serialplugin/commit/09e6a329833281759fbe993602b7d23fda2c2f86))
* **serial:** add readBinary method for reading binary data ([5a62544](https://github.com/s00d/tauri-plugin-serialplugin/commit/5a62544e554ac3098e9ca99f3211ba0a5097aa70))

## [2.7.0](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.6.1...v2.7.0) (2025-01-27)


### Features

* **mobile_api:** update managed_ports to return a list of port names ([77e6799](https://github.com/s00d/tauri-plugin-serialplugin/commit/77e67998b294ee2599c8a765798b67815f4bbd84))

### [2.6.1](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.6.0...v2.6.1) (2025-01-27)

## [2.6.0](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.4.13...v2.6.0) (2025-01-14)


### Features

* **app:** add managed ports functionality and UI integration ([a3465ce](https://github.com/s00d/tauri-plugin-serialplugin/commit/a3465ced1ca053d31768600ef40ab4b853c68728))
* **build:** add managed_ports command to the serial plugin ([6a79bbb](https://github.com/s00d/tauri-plugin-serialplugin/commit/6a79bbb20d1eda64600b2609f21838cb8a1fbc14))
* **permissions:** add managed_ports command permissions ([634b713](https://github.com/s00d/tauri-plugin-serialplugin/commit/634b713d551295bad502746a2c80157eb0ce4d63))
* **permissions:** add managed_ports permissions and update documentation ([9714453](https://github.com/s00d/tauri-plugin-serialplugin/commit/97144537f153759d9ba9f2bb6efe370eaa6b726b))
* **README:** add documentation for managed ports feature ([b241c13](https://github.com/s00d/tauri-plugin-serialplugin/commit/b241c1317c9a591c54d9020eb955b06cc990d8ac))
* **schemas:** add commands for webview and window background color ([67b9670](https://github.com/s00d/tauri-plugin-serialplugin/commit/67b9670b9df0fe6036363aec6141438625a7ecc3))
* **serial:** add managed_ports command to list open serial ports ([7eb727d](https://github.com/s00d/tauri-plugin-serialplugin/commit/7eb727dbfa19234f8c4a4749f518ac38463c61ac))
* **serialplugin:** add managedPorts command to retrieve active ports ([378ee2a](https://github.com/s00d/tauri-plugin-serialplugin/commit/378ee2a9534f9dead7ca6f7b4ad59126996282af))
* **serialplugin:** add method to list all managed serial ports ([d04b991](https://github.com/s00d/tauri-plugin-serialplugin/commit/d04b9913914c6a9af889c895a28a532a79d160de))

### [2.4.13](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.4.11...v2.4.13) (2024-12-24)


### Bug Fixes

* **package:** update version numbers in package files to 2.4.12 ([323a1f9](https://github.com/s00d/tauri-plugin-serialplugin/commit/323a1f98c4ce4ec035d3594210250622ba869823))

### [2.4.11](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.4.10...v2.4.11) (2024-11-29)

### [2.4.10](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.4.9...v2.4.10) (2024-11-29)

### [2.4.9](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.4.7...v2.4.9) (2024-11-29)

### [2.4.7](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.4.6...v2.4.7) (2024-11-29)

### [2.4.6](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.4.5...v2.4.6) (2024-11-29)

### [2.4.4](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.4.5...v2.4.4) (2024-11-29)

### [2.4.5](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.4.4...v2.4.5) (2024-11-29)

### [2.4.4](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.1.0...v2.4.4) (2024-11-29)


### Bug Fixes

* size and timeout settings not working. ([e49197d](https://github.com/s00d/tauri-plugin-serialplugin/commit/e49197d876f88e1a5b5f6f6e15dfd7d2a90e3617))

## [2.3.1] - 2024-11-10

### Added
- Automatic cleanup of existing listeners before starting new ones

## [2.3.0] - 2024-11-10

### Added
- New `startListening` command for explicit port monitoring control
- New `stopListening` command for manual monitoring termination

## [2.2.0] - 2024-11-10

### Added
- Automatic port listening on connection
- Background thread management for port monitoring

### Changed
- Refactored read operation to be synchronous instead of event-based
- Improved port cleanup on close
- Modified TypeScript interface to return string data directly from read operation
- Changed port reading logic to use direct synchronous reads
- Added automatic port monitoring on connection

## [2.1.0] - 2024-11-01

### Added
- New serial port control methods:
    - `set_baud_rate`: Set the baud rate
    - `set_data_bits`: Set the data bits configuration
    - `set_flow_control`: Set the flow control mode
    - `set_parity`: Set the parity checking mode
    - `set_stop_bits`: Set the stop bits configuration
    - `set_timeout`: Set the timeout duration
    - `write_request_to_send`: Set RTS control signal
    - `write_data_terminal_ready`: Set DTR control signal
    - `read_clear_to_send`: Read CTS signal state
    - `read_data_set_ready`: Read DSR signal state
    - `read_ring_indicator`: Read RI signal state
    - `read_carrier_detect`: Read CD signal state
    - `bytes_to_read`: Get available bytes to read
    - `bytes_to_write`: Get bytes waiting to be written
    - `clear_buffer`: Clear input/output buffers
    - `set_break`: Start break signal
    - `clear_break`: Stop break signal
- New permissions for all added methods
- Enhanced error handling for serial port operations

### Changed
- Improved error handling system
- Enhanced documentation for all methods
- Updated TypeScript definitions with JSDoc comments

### Fixed
- Error conversion between serialport and internal errors
- Type conversion issues in serial port operations

## [2.0.2] - 2023-12-20

### Added
- Support for direct port scanning on Windows, Linux, and macOS

### Changed
- Updated dependencies to latest versions
- Improved error messages

### Fixed
- Port detection issues on various platforms

## [2.0.1] - 2023-12-10

### Changed
- Updated dependencies
- Improved available_ports_direct logic
- Updated test UI

## [2.0.0-rc.3] - 2023-12-01

### Added
- Initial implementation of available_ports_direct
