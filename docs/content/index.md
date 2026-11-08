---
title: tauri-plugin-serialplugin
description: Serial ports for Tauri 2 apps on desktop, Android and iOS, over USB, TCP and Bluetooth LE.
navigation: false
---

```ts [main.ts]
import { SerialPort } from "tauri-plugin-serialplugin-api";

const ports = await SerialPort.available_ports();   // { "/dev/ttyUSB0": { type: "USB", vid, pid, … }, … }

const port = new SerialPort({ path: Object.keys(ports)[0], baudRate: 115200 });
await port.open();

const handle = await port.watch({
  onData: (text) => console.log("rx", text),
  onDisconnect: (reason) => console.log("gone:", reason),
});

await port.write("hello\n");

await handle.unwatch();
await port.close();
```
