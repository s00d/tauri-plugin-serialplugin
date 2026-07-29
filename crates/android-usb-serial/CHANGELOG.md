# Changelog

## Unreleased

- Track CDC ACM interrupt-IN notifications and expose DCD, DSR, and ring state
  through the modem-status API

## 0.1.0

- Initial release: FTDI, CP21xx, CH34x, Prolific, CDC-ACM, GSM modem, Chrome CCD drivers on `nusb`
- `from_raw_fd` / `NusbTransport` for Android `UsbDeviceConnection` fds
- `ProbeTable`, `SerialPortHandle`, optional `serialport-compat` and `fake-transport`
- Golden parity fixtures (≥560 Java-sourced control sequences)
