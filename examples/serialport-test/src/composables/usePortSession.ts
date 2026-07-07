import { ref, shallowRef, type Ref } from 'vue';
import {
  ClearBuffer,
  DataBits,
  FlowControl,
  Parity,
  SerialPort,
  StopBits,
  type WatchHandle,
} from 'tauri-plugin-serialplugin-api';
import type { LineEnding, SendMode, TerminalLine, TerminalLineKind, WatchLogEntry } from '../types';
import type { AtEntry } from '../components/PortAtConsole.vue';

let logId = 0;
let terminalId = 0;
let atEntryId = 0;

function timeLabel(): string {
  return new Date().toLocaleTimeString(undefined, {
    hour12: false,
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  });
}

function lineEndingSuffix(mode: LineEnding): string {
  switch (mode) {
    case 'lf':
      return '\n';
    case 'cr':
      return '\r';
    case 'crlf':
      return '\r\n';
    default:
      return '';
  }
}

function bytesToHex(data: Uint8Array): string {
  return [...data].map((b) => b.toString(16).padStart(2, '0')).join(' ');
}

function parseHexInput(raw: string): Uint8Array | null {
  const cleaned = raw.replace(/0x/gi, '').replace(/[^0-9a-fA-F]/g, '');
  if (!cleaned || cleaned.length % 2 !== 0) return null;
  const out = new Uint8Array(cleaned.length / 2);
  for (let i = 0; i < out.length; i++) {
    out[i] = parseInt(cleaned.slice(i * 2, i * 2 + 2), 16);
  }
  return out;
}

export function usePortSession(portPath: Ref<string | null>) {
  const port = shallowRef<SerialPort | null>(null);
  const connected = ref(false);
  const watching = ref(false);
  const watchHandle = ref<WatchHandle | null>(null);
  const channelId = ref<number | null>(null);
  const lastError = ref<string | null>(null);
  const events = ref<WatchLogEntry[]>([]);
  const terminalLines = ref<TerminalLine[]>([]);
  const bytesToRead = ref(0);
  const bytesToWrite = ref(0);

  const baudRate = ref(115200);
  const dataBits = ref(DataBits.Eight);
  const flowControl = ref(FlowControl.None);
  const parity = ref(Parity.None);
  const stopBits = ref(StopBits.One);
  const timeout = ref(1000);
  const flushMs = ref(100);

  const autoReconnect = ref(false);
  const autoReconnectInfo = ref<ReturnType<SerialPort['getAutoReconnectInfo']> | null>(null);

  const atBusy = ref(false);
  const atEntries = ref<AtEntry[]>([]);

  const rts = ref(false);
  const dtr = ref(false);
  const cts = ref(false);
  const dsr = ref(false);
  const ri = ref(false);
  const cd = ref(false);

  function refreshAutoReconnectInfo() {
    autoReconnectInfo.value = port.value?.getAutoReconnectInfo() ?? null;
  }

  function pushTerminal(kind: TerminalLineKind, text: string) {
    terminalLines.value.push({ id: ++terminalId, time: timeLabel(), kind, text });
    if (terminalLines.value.length > 500) {
      terminalLines.value.splice(0, terminalLines.value.length - 500);
    }
  }

  function log(kind: WatchLogEntry['kind'], message: string) {
    events.value = [
      { id: ++logId, time: timeLabel(), kind, message },
      ...events.value,
    ].slice(0, 200);

    const terminalKind: TerminalLineKind =
      kind === 'data'
        ? 'rx'
        : kind === 'error'
          ? 'err'
          : kind === 'disconnect'
            ? 'disconnect'
            : 'sys';
    if (kind !== 'data') {
      pushTerminal(terminalKind, message);
    }
  }

  function formatTerminalText(text: string): string {
    return text.replace(/\r\n/g, '\n').replace(/\r/g, '\n');
  }

  function appendRx(data: string | Uint8Array) {
    const raw =
      typeof data === 'string'
        ? data
        : [...data].map((b) => (b >= 32 && b < 127 ? String.fromCharCode(b) : `\\x${b.toString(16).padStart(2, '0')}`)).join('');
    rxBatch += raw;
    scheduleRxFlush();
  }

  async function refreshSignals() {
    if (!port.value || !connected.value || watching.value) return;
    try {
      bytesToRead.value = await port.value.bytesToRead();
      bytesToWrite.value = await port.value.bytesToWrite();
      cts.value = await port.value.readClearToSend();
      dsr.value = await port.value.readDataSetReady();
      ri.value = await port.value.readRingIndicator();
      cd.value = await port.value.readCarrierDetect();
    } catch (e) {
      log('error', `Status: ${String(e)}`);
    }
  }

  let rxBatch = '';
  let rxFlushHandle: number | null = null;

  function flushRxBatch() {
    rxFlushHandle = null;
    if (!rxBatch) return;
    const chunk = formatTerminalText(rxBatch);
    rxBatch = '';
    pushTerminal('rx', chunk);
    log('data', chunk.length > 80 ? `${chunk.slice(0, 80)}… (${chunk.length} B)` : chunk);
  }

  function scheduleRxFlush() {
    if (rxFlushHandle != null) return;
    rxFlushHandle = requestAnimationFrame(flushRxBatch);
  }

  async function connect() {
    if (!portPath.value || connected.value) return;
    lastError.value = null;
    try {
      const instance = new SerialPort({
        path: portPath.value,
        baudRate: baudRate.value,
        dataBits: dataBits.value,
        flowControl: flowControl.value,
        parity: parity.value,
        stopBits: stopBits.value,
        timeout: timeout.value,
        atSession: {
          defaultTimeoutMs: timeout.value,
          expectOk: false,
        },
      });

      await instance.open();
      port.value = instance;
      connected.value = true;
      pushTerminal('sys', `Opened ${portPath.value} @ ${baudRate.value} baud`);

      if (autoReconnect.value) {
        instance.enableAutoReconnect({
          maxAttempts: null,
          onReconnect: (ok) => {
            pushTerminal('sys', ok ? 'Auto-reconnect OK' : 'Auto-reconnect failed');
            refreshAutoReconnectInfo();
          },
        });
      }
      refreshAutoReconnectInfo();

      const handle = await instance.watch(
        {
          onData: (data) => {
            appendRx(data);
          },
          onUrc: (line) => {
            pushTerminal('urc', line);
            log('info', `URC: ${line}`);
          },
          onError: (message) => {
            lastError.value = message;
            log('error', message);
          },
          onDisconnect: (reason) => {
            connected.value = false;
            watching.value = false;
            watchHandle.value = null;
            channelId.value = null;
            log('disconnect', reason);
          },
        },
        { serialDataFlushIntervalMs: flushMs.value, timeout: timeout.value },
      );

      watchHandle.value = handle;
      channelId.value = handle.channelId;
      watching.value = true;
      pushTerminal('sys', `Watch started (channel ${handle.channelId})`);
      await refreshSignals();
    } catch (e) {
      lastError.value = String(e);
      log('error', `Connect: ${String(e)}`);
    }
  }

  async function disconnect() {
    try {
      port.value?.cancelAt();
      if (watchHandle.value) {
        await watchHandle.value.unwatch();
        watchHandle.value = null;
        watching.value = false;
        channelId.value = null;
      }
      if (port.value) {
        port.value.disableAutoReconnect();
        await port.value.close();
        port.value = null;
      }
      connected.value = false;
      atBusy.value = false;
      pushTerminal('sys', 'Port closed');
    } catch (e) {
      log('error', `Disconnect: ${String(e)}`);
    }
  }

  async function applySettings() {
    if (!port.value || !connected.value) return;
    try {
      await port.value.setBaudRate(baudRate.value);
      await port.value.setDataBits(dataBits.value);
      await port.value.setFlowControl(flowControl.value);
      await port.value.setParity(parity.value);
      await port.value.setStopBits(stopBits.value);
      await port.value.setTimeout(timeout.value);
      await port.value.configureAtSession({
        defaultTimeoutMs: timeout.value,
        expectOk: false,
      });
      pushTerminal('sys', 'Settings applied');
    } catch (e) {
      log('error', `Settings: ${String(e)}`);
    }
  }

  async function sendPayload(
    raw: string,
    options: { mode: SendMode; lineEnding: LineEnding; localEcho: boolean },
  ) {
    if (!port.value || !raw.trim()) return;

    if (options.mode === 'at') {
      const cmd = raw.trim();
      pushTerminal('tx', `AT> ${cmd}`);
      void runAt(cmd);
      return;
    }

    if (options.mode === 'hex') {
      const bytes = parseHexInput(raw);
      if (!bytes) {
        pushTerminal('err', 'Invalid hex (use pairs like 48 65 6c 6c 6f)');
        return;
      }
      try {
        await port.value.writeBinary(bytes);
        if (options.localEcho) {
          pushTerminal('tx', bytesToHex(bytes));
        }
        await refreshSignals();
      } catch (e) {
        log('error', `Write hex: ${String(e)}`);
      }
      return;
    }

    const payload = raw + lineEndingSuffix(options.lineEnding);
    try {
      await port.value.write(payload);
      if (options.localEcho) {
        pushTerminal('tx', raw);
      }
      await refreshSignals();
    } catch (e) {
      log('error', `Write: ${String(e)}`);
    }
  }

  async function sendBinaryDemo() {
    if (!port.value) return;
    try {
      const payload = new Uint8Array([0x48, 0x69, 0x0a]);
      await port.value.writeBinary(payload);
      pushTerminal('tx', bytesToHex(payload));
      await refreshSignals();
    } catch (e) {
      log('error', `Write binary: ${String(e)}`);
    }
  }

  async function pollRead() {
    if (!port.value) return;
    try {
      const data = await port.value.read({ timeout: 500, size: 256 });
      pushTerminal('sys', `Poll read: ${data || '(empty)'}`);
      await refreshSignals();
    } catch (e) {
      log('error', `Read: ${String(e)}`);
    }
  }

  async function clearBuffers() {
    if (!port.value) return;
    try {
      await port.value.clearBuffer(ClearBuffer.All);
      pushTerminal('sys', 'Buffers cleared');
      await refreshSignals();
    } catch (e) {
      log('error', `Clear: ${String(e)}`);
    }
  }

  async function toggleRts() {
    if (!port.value) return;
    rts.value = !rts.value;
    await port.value.writeRequestToSend(rts.value);
    pushTerminal('sys', `RTS ${rts.value ? 'ON' : 'off'}`);
  }

  async function toggleDtr() {
    if (!port.value) return;
    dtr.value = !dtr.value;
    await port.value.writeDataTerminalReady(dtr.value);
    pushTerminal('sys', `DTR ${dtr.value ? 'ON' : 'off'}`);
  }

  function clearTerminal() {
    terminalLines.value = [];
    events.value = [];
  }

  async function runAt(command: string) {
    if (!port.value || !connected.value) return;

    const id = ++atEntryId;
    atEntries.value = [{ id, command, status: 'running' as const }, ...atEntries.value].slice(0, 50);
    atBusy.value = true;

    try {
      const result = await port.value.sendAt(command, { timeoutMs: timeout.value });
      atEntries.value = atEntries.value.map((entry) =>
        entry.id === id
          ? {
              ...entry,
              status: 'done' as const,
              response: result.response,
              parseStatus: result.status,
              urcLines: result.urcLines,
            }
          : entry,
      );
      pushTerminal('at', result.response.trim() || '(empty response)');
      for (const line of result.urcLines) {
        pushTerminal('urc', line);
      }
    } catch (e) {
      atEntries.value = atEntries.value.map((entry) =>
        entry.id === id ? { ...entry, status: 'error' as const, error: String(e) } : entry,
      );
      pushTerminal('err', `AT ${command}: ${String(e)}`);
    } finally {
      atBusy.value = false;
    }
  }

  function cancelAt() {
    if (!port.value) return;
    void port.value.cancelAt();
    atEntries.value = atEntries.value.map((entry) =>
      entry.status === 'running' ? { ...entry, status: 'error' as const, error: 'cancelled' } : entry,
    );
    atBusy.value = false;
    pushTerminal('sys', 'AT cancelled');
  }

  async function runAtScript(lines: string[]) {
    if (!port.value || !connected.value || lines.length === 0) return;

    const batch = lines.map((command) => ({ id: ++atEntryId, command }));
    atEntries.value = [
      ...batch.map(({ id, command }) => ({ id, command, status: 'running' as const })),
      ...atEntries.value,
    ].slice(0, 50);
    atBusy.value = true;

    try {
      const results = await port.value.sendAtPhases(
        batch.map(({ command }) => ({ write: command, command })),
      );
      for (let i = 0; i < batch.length; i++) {
        const { id, command } = batch[i];
        const result = results[i];
        atEntries.value = atEntries.value.map((entry) =>
          entry.id === id
            ? {
                ...entry,
                status: 'done' as const,
                response: result.response,
                parseStatus: result.status,
                urcLines: result.urcLines,
              }
            : entry,
        );
        pushTerminal('at', `${command}: ${result.response.trim() || '(empty)'}`);
        for (const line of result.urcLines) {
          pushTerminal('urc', line);
        }
      }
    } catch (e) {
      const msg = String(e);
      atEntries.value = atEntries.value.map((entry) =>
        batch.some((b) => b.id === entry.id && entry.status === 'running')
          ? { ...entry, status: 'error' as const, error: msg }
          : entry,
      );
      pushTerminal('err', `AT script: ${msg}`);
    } finally {
      atBusy.value = false;
    }
  }

  function resetForPath() {
    void disconnect();
    clearTerminal();
    atEntries.value = [];
    atBusy.value = false;
    lastError.value = null;
  }

  return {
    port,
    connected,
    watching,
    channelId,
    lastError,
    events,
    terminalLines,
    bytesToRead,
    bytesToWrite,
    baudRate,
    dataBits,
    flowControl,
    parity,
    stopBits,
    timeout,
    flushMs,
    autoReconnect,
    autoReconnectInfo,
    refreshAutoReconnectInfo,
    rts,
    dtr,
    cts,
    dsr,
    ri,
    cd,
    connect,
    disconnect,
    applySettings,
    sendPayload,
    sendBinaryDemo,
    pollRead,
    clearBuffers,
    toggleRts,
    toggleDtr,
    refreshSignals,
    clearTerminal,
    resetForPath,
    log,
    atBusy,
    atEntries,
    cancelAt,
    runAtScript,
  };
}
