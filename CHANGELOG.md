# Changelog

All notable changes to this project will be documented in this file. See [standard-version](https://github.com/conventional-changelog/standard-version) for commit guidelines.

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
