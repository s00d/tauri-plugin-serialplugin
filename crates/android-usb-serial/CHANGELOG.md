# Changelog

## Unreleased

- Return `Disconnected` for I/O and control operations on closed serial handles,
  preventing stale adapter clones from panicking after USB detach.
- Add regression coverage for stale clones, closed-handle operations, and reopening.

## 0.1.1

- Track CDC ACM interrupt-IN notifications and expose DCD, DSR, and ring state
  through the modem-status API
- Skip malformed / unrelated CDC interrupt frames instead of killing the
  notification reader (match usb-serial-for-android)

## 0.1.0

- Initial release: FTDI, CP21xx, CH34x, Prolific, CDC-ACM, GSM modem, Chrome CCD drivers on `nusb`
- `from_raw_fd` / `NusbTransport` for Android `UsbDeviceConnection` fds
- `ProbeTable`, `SerialPortHandle`, optional `serialport-compat` and `fake-transport`
- Golden parity fixtures (≥560 Java-sourced control sequences)
