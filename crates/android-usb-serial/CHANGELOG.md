# Changelog

## 0.1.3

- PL2303: detect the chip type from the device descriptor. `raw_descriptors()` holds
  configuration descriptors on real devices, so every PL2303 (HXN/GC/GS/GT included)
  was treated as the legacy Type01 and got the wrong init and baud encoding.
- FTDI: read the device type (bcdDevice) from the device descriptor, so FT232H gets
  the port-indexed baud divisor.
- FTDI: baud rate 0 returns `Unsupported` instead of panicking on a division by zero.
- MSRV is 1.85 (what `nusb` needs; 1.79 was never buildable).
- CDC: a data interface without an ACM control interface is no longer a serial port
  (USB Ethernet, RNDIS); an out-of-range port index is an error; notification errors
  stay reported until close.
- `clear(Input)` also drops chunks the reader already queued; `try_read` keeps the
  rest of a chunk larger than the buffer; a timed-out bulk write reports the bytes it
  did send.
- FTDI: `set_latency_timer` (driver trait, no-op elsewhere). CH34x: RTS/CTS flow
  control and the 307200 baud divisor. PL2303: dropped direct rates older chips
  reject. FT4232H fallback port count 4.

## 0.1.2

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
