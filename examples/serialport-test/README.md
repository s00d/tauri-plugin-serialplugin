# serialport-test (v3)

Demo for **tauri-plugin-serialplugin** v3: terminal UI, `watch()` / `watchAvailablePorts()`, AT queue, signals, config.

## Layout

- **Status bar** — capabilities, log level, Rust `get_ports_programmatically` (desktop)
- **Port picker** — live hotplug (`watchAvailablePorts`), manual path, managed ports
- **Terminal** — scrollable RX/TX/URC/sys log; **input dock at the bottom** (Text / AT / Hex, line endings)
- **Tools panel** — Signals (RTS/DTR/CTS…), AT script queue, port config

## Terminal

1. Select a port → **Connect**
2. Type at the bottom → Enter or **Send**
3. Modes: **Text** (raw write), **AT** (`sendAt()`), **Hex** (`48 65 6c 6c 6f`)
4. Toolbar: clear, poll read, demo binary, flush buffers

## Mobile

- Port list opens as a slide-over drawer (☰ / **Ports**)
- Terminal input respects safe-area inset; font 16px to avoid iOS zoom
- **Tools** collapses above the input dock

## Dev

```bash
pnpm install
pnpm tauri dev
```

In dev, Vite resolves `tauri-plugin-serialplugin-api` to **`guest-js/`**. Clear cache after major API changes:

```bash
pnpm run clean:vite
pnpm tauri dev
```
