import { Channel, invoke } from '@tauri-apps/api/core';
import {
  setLogLevel as setInternalLogLevel,
  logError,
  logWarn,
  logInfo,
  type LogLevel,
} from './logger';
import {
  DEFAULT_SERIAL_TIMEOUT_MS,
  DataBits,
  FlowControl,
  Parity,
  StopBits,
  type AutoReconnectInfo,
  type AutoReconnectOptions,
  type AvailablePortsOptions,
  type BaudRate,
  type Capabilities,
  type ClearBuffer,
  type ExchangeOptions,
  type ExchangeResponse,
  type Options,
  type PortInfo,
  type PortListEvent,
  type ReadOptions,
  type SerialEvent,
  type SerialportOptions,
  type WatchHandle,
  type WatchHandlers,
  type WatchOptions,
  type WatchPortsHandlers,
  type WatchPortsOptions,
  type AtSessionOptions,
  type AtCommandOptions,
  type AtCommandResult,
  type AtPhase,
  type SendSmsPduOptions,
} from './types';

let capabilitiesCache: Capabilities | null = null;

/** @internal */
export function resetCapabilitiesCacheForTests(): void {
  capabilitiesCache = null;
}

function textDecoder(encoding: string): TextDecoder {
  try {
    return new TextDecoder(encoding, { fatal: false });
  } catch {
    return new TextDecoder('utf-8', { fatal: false });
  }
}

function dispatchWatchData(
  data: number[],
  decoder: TextDecoder,
  decode: boolean,
  onData: (data: string | Uint8Array) => void,
): void {
  const bytes = new Uint8Array(data);
  if (!decode) {
    onData(bytes);
    return;
  }
  try {
    const text = decoder.decode(bytes, { stream: true });
    if (text.length > 0) {
      onData(text);
    }
  } catch (error) {
    logError('Error decoding serial data:', error);
    onData(bytes);
  }
}

async function invokeWatch(
  path: string,
  handlers: WatchHandlers,
  options: WatchOptions | undefined,
  encoding: string,
): Promise<WatchHandle> {
  const decode = options?.decode !== false;
  const decoder = textDecoder(encoding);
  const channel = new Channel<SerialEvent>();

  channel.onmessage = (event) => {
    if (event.path !== path) {
      return;
    }
    switch (event.kind) {
      case 'data':
        dispatchWatchData(event.data, decoder, decode, handlers.onData);
        break;
      case 'urc':
        handlers.onUrc?.(event.line);
        break;
      case 'disconnect':
        handlers.onDisconnect?.(event.reason);
        break;
      case 'error':
        handlers.onError?.(event.message);
        break;
    }
  };

  const watchOptions: Record<string, unknown> = {};
  if (options?.timeout != null) {
    watchOptions.timeout = options.timeout;
  }
  if (options?.size != null) {
    watchOptions.size = options.size;
  }
  if (options?.serialDataFlushIntervalMs != null) {
    watchOptions.serialDataFlushIntervalMs = options.serialDataFlushIntervalMs;
  }

  const channelId = await invoke<number>('plugin:serialplugin|watch', {
    path,
    options: watchOptions,
    channel,
  });

  return {
    channelId,
    unwatch: () => invoke<void>('plugin:serialplugin|unwatch', { channelId }),
  };
}

export class SerialPort {
  isOpen: boolean;
  encoding: string;
  options: Options;

  private watchHandle: WatchHandle | null = null;
  private lastWatchHandlers: WatchHandlers | null = null;
  private lastWatchOptions: WatchOptions | undefined;
  private isProcessingOpenClose = false;
  private autoReconnectEnabled = false;
  private autoReconnectInterval = 5000;
  private autoReconnectMaxAttempts: number | null = 10;
  private autoReconnectAttempts = 0;
  private autoReconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private autoReconnectCallback?: (success: boolean, attempt: number) => void;
  private readonly atSessionOptions?: AtSessionOptions;

  constructor(options: SerialportOptions) {
    this.isOpen = false;
    this.encoding = options.encoding || 'utf-8';
    this.atSessionOptions = options.atSession;
    this.options = {
      path: options.path,
      baudRate: options.baudRate,
      dataBits: options.dataBits || DataBits.Eight,
      flowControl: options.flowControl || FlowControl.None,
      parity: options.parity || Parity.None,
      stopBits: options.stopBits || StopBits.One,
      size: options.size || 1024,
      timeout: options.timeout ?? DEFAULT_SERIAL_TIMEOUT_MS,
      ...(options.serialDataFlushIntervalMs != null && {
        serialDataFlushIntervalMs: options.serialDataFlushIntervalMs,
      }),
    };
  }

  get size(): number {
    return this.options.size ?? 1024;
  }

  /** Active watch handle, if any. */
  get activeWatch(): WatchHandle | null {
    return this.watchHandle;
  }

  static async getCapabilities(): Promise<Capabilities> {
    if (!capabilitiesCache) {
      capabilitiesCache = await invoke<Capabilities>('plugin:serialplugin|capabilities');
    }
    return capabilitiesCache;
  }

  static async available_ports(
    options?: AvailablePortsOptions,
  ): Promise<Record<string, PortInfo>> {
    return invoke<Record<string, PortInfo>>('plugin:serialplugin|available_ports', {
      singlePortPerDevice: options?.singlePortPerDevice ?? false,
    });
  }

  /**
   * Subscribe to available-port hotplug (attach/detach). Sends an initial snapshot,
   * then `added` / `removed` events. Call `unwatch()` on the returned handle to stop.
   */
  static async watchAvailablePorts(
    handlers: WatchPortsHandlers,
    options?: WatchPortsOptions,
  ): Promise<WatchHandle> {
    const channel = new Channel<PortListEvent>();
    channel.onmessage = (event) => {
      switch (event.kind) {
        case 'snapshot':
          handlers.onSnapshot?.(event.ports);
          break;
        case 'added':
          handlers.onAdded?.(event.path, event.info);
          break;
        case 'removed':
          handlers.onRemoved?.(event.path);
          break;
      }
    };

    const watchOptions: Record<string, unknown> = {};
    if (options?.singlePortPerDevice != null) {
      watchOptions.singlePortPerDevice = options.singlePortPerDevice;
    }
    if (options?.pollIntervalMs != null) {
      watchOptions.pollIntervalMs = options.pollIntervalMs;
    }

    const channelId = await invoke<number>('plugin:serialplugin|watch_ports', {
      options: watchOptions,
      channel,
    });

    return {
      channelId,
      unwatch: () => invoke<void>('plugin:serialplugin|unwatch_ports', { channelId }),
    };
  }

  static async managed_ports(): Promise<string[]> {
    return invoke<string[]>('plugin:serialplugin|managed_ports');
  }

  static async forceClose(path: string): Promise<void> {
    return invoke<void>('plugin:serialplugin|force_close', { path });
  }

  static async closeAll(): Promise<void> {
    return invoke<void>('plugin:serialplugin|close_all');
  }

  static async setLogLevel(level: LogLevel): Promise<void> {
    await invoke<void>('plugin:serialplugin|set_log_level', { level });
    const synced = await invoke<LogLevel>('plugin:serialplugin|get_log_level');
    setInternalLogLevel(synced);
  }

  static async getLogLevel(): Promise<LogLevel> {
    const level = await invoke<LogLevel>('plugin:serialplugin|get_log_level');
    setInternalLogLevel(level);
    return level;
  }

  private buildWatchHandlers(handlers: WatchHandlers): WatchHandlers {
    return {
      onData: handlers.onData,
      onError: handlers.onError,
      onDisconnect: (reason) => {
        void this.handleDisconnect(reason, handlers.onDisconnect);
      },
    };
  }

  async watch(
    handlers: WatchHandlers,
    options?: WatchOptions,
  ): Promise<WatchHandle> {
    if (!this.isOpen) {
      throw new Error('Port is not open');
    }
    if (!this.options.path) {
      throw new Error('path cannot be empty');
    }
    if (this.watchHandle) {
      throw new Error('A watch is already active on this port instance');
    }

    const resolvedOptions: WatchOptions = {
      timeout: options?.timeout ?? this.options.timeout,
      size: options?.size ?? this.options.size,
      serialDataFlushIntervalMs:
        options?.serialDataFlushIntervalMs ?? this.options.serialDataFlushIntervalMs,
      decode: options?.decode,
    };

    this.lastWatchHandlers = {
      onData: handlers.onData,
      onError: handlers.onError,
      onDisconnect: handlers.onDisconnect,
    };
    this.lastWatchOptions = resolvedOptions;

    const path = this.options.path;
    const handle = await invokeWatch(
      path,
      this.buildWatchHandlers(handlers),
      resolvedOptions,
      this.encoding,
    );

    this.watchHandle = handle;
    return handle;
  }

  private clearSavedWatchSession(): void {
    this.lastWatchHandlers = null;
    this.lastWatchOptions = undefined;
  }

  private async handleDisconnect(
    reason: string,
    userCallback?: (reason: string) => void,
  ): Promise<void> {
    this.isOpen = false;
    userCallback?.(reason);
    await this.stopWatch();
    this.scheduleAutoReconnect();
  }

  private async reestablishWatch(): Promise<void> {
    if (!this.isOpen || !this.lastWatchHandlers || this.watchHandle) {
      return;
    }
    if (!this.options.path) {
      return;
    }

    const path = this.options.path;
    const saved = this.lastWatchHandlers;
    const handle = await invokeWatch(
      path,
      this.buildWatchHandlers(saved),
      this.lastWatchOptions,
      this.encoding,
    );
    this.watchHandle = handle;
  }

  private clearAutoReconnectTimer(): void {
    if (this.autoReconnectTimer) {
      clearTimeout(this.autoReconnectTimer);
      this.autoReconnectTimer = null;
    }
  }

  private scheduleAutoReconnect(): void {
    if (!this.autoReconnectEnabled || this.isOpen) {
      return;
    }
    this.autoReconnectAttempts = 0;
    void this.runAutoReconnectAttempt();
  }

  private async runAutoReconnectAttempt(): Promise<void> {
    if (!this.autoReconnectEnabled || this.isOpen) {
      return;
    }
    if (
      this.autoReconnectMaxAttempts !== null &&
      this.autoReconnectAttempts >= this.autoReconnectMaxAttempts
    ) {
      logError(`Auto-reconnect failed after ${this.autoReconnectMaxAttempts} attempts`);
      this.autoReconnectCallback?.(false, this.autoReconnectAttempts);
      return;
    }

    this.autoReconnectAttempts += 1;
    logInfo(
      `Auto-reconnect attempt ${this.autoReconnectAttempts}${
        this.autoReconnectMaxAttempts !== null ? `/${this.autoReconnectMaxAttempts}` : ''
      }`,
    );

    try {
      await this.open();
      await this.reestablishWatch();
      this.autoReconnectAttempts = 0;
      logInfo('Auto-reconnect successful');
      this.autoReconnectCallback?.(true, 0);
    } catch (error) {
      logError(`Auto-reconnect attempt ${this.autoReconnectAttempts} failed:`, error);
      this.autoReconnectCallback?.(false, this.autoReconnectAttempts);
      if (this.autoReconnectEnabled) {
        this.autoReconnectTimer = setTimeout(() => {
          void this.runAutoReconnectAttempt();
        }, this.autoReconnectInterval);
      }
    }
  }

  private async stopWatch(): Promise<void> {
    if (!this.watchHandle) {
      return;
    }
    try {
      await this.watchHandle.unwatch();
    } catch (error) {
      logWarn('Error stopping watch:', error);
    } finally {
      this.watchHandle = null;
    }
  }

  async change(options: { path?: string; baudRate?: number }): Promise<void> {
    const wasOpen = this.isOpen;
    if (wasOpen) {
      await this.close();
    }
    if (options.path) {
      this.options.path = options.path;
    }
    if (options.baudRate) {
      this.options.baudRate = options.baudRate;
    }
    if (wasOpen) {
      await this.open();
    }
  }

  async close(): Promise<void> {
    if (!this.isOpen) {
      return;
    }
    if (this.isProcessingOpenClose) {
      throw new Error('Serial port open/close already in progress');
    }
    this.isProcessingOpenClose = true;
    this.clearAutoReconnectTimer();
    this.clearSavedWatchSession();

    try {
      await this.stopWatch();
      try {
        await invoke<void>('plugin:serialplugin|cancel_read', {
          path: this.options.path,
        });
      } catch (error) {
        logWarn('Error during cancelRead:', error);
      }
      try {
        await invoke<void>('plugin:serialplugin|close', {
          path: this.options.path,
        });
      } catch (error) {
        logWarn('Error during port close:', error);
      }
    } finally {
      this.isOpen = false;
      this.isProcessingOpenClose = false;
    }
  }

  enableAutoReconnect(options: AutoReconnectOptions = {}): void {
    this.autoReconnectEnabled = true;
    this.autoReconnectInterval = options.interval ?? 5000;
    this.autoReconnectMaxAttempts =
      options.maxAttempts === undefined ? 10 : options.maxAttempts;
    this.autoReconnectCallback = options.onReconnect;
    this.autoReconnectAttempts = 0;
  }

  disableAutoReconnect(): void {
    this.autoReconnectEnabled = false;
    this.clearAutoReconnectTimer();
    this.autoReconnectAttempts = 0;
    this.autoReconnectCallback = undefined;
    this.clearSavedWatchSession();
  }

  getAutoReconnectInfo(): AutoReconnectInfo {
    return {
      enabled: this.autoReconnectEnabled,
      interval: this.autoReconnectInterval,
      maxAttempts: this.autoReconnectMaxAttempts,
      currentAttempts: this.autoReconnectAttempts,
      hasCallback: !!this.autoReconnectCallback,
    };
  }

  async manualReconnect(): Promise<boolean> {
    if (this.isOpen) {
      return true;
    }
    try {
      await this.open();
      if (this.lastWatchHandlers) {
        await this.reestablishWatch();
      }
      return true;
    } catch (error) {
      logError('Manual reconnection failed:', error);
      return false;
    }
  }

  async open(): Promise<void> {
    if (!this.options.path) {
      throw new Error('path cannot be empty');
    }
    if (!this.options.baudRate) {
      throw new Error('baudRate cannot be empty');
    }
    if (this.isOpen) {
      return;
    }
    if (this.isProcessingOpenClose) {
      throw new Error('Serial port open/close already in progress');
    }
    this.isProcessingOpenClose = true;
    try {
      await invoke<void>('plugin:serialplugin|open', {
        path: this.options.path,
        baudRate: this.options.baudRate,
        dataBits: this.options.dataBits,
        flowControl: this.options.flowControl,
        parity: this.options.parity,
        stopBits: this.options.stopBits,
        timeout: this.options.timeout,
      });
      this.isOpen = true;
      if (this.atSessionOptions) {
        await this.configureAtSession(this.atSessionOptions);
      }
    } finally {
      this.isProcessingOpenClose = false;
    }
  }

  async read(options?: ReadOptions): Promise<string> {
    if (!this.isOpen) {
      throw new Error('Port is not open');
    }
    return invoke<string>('plugin:serialplugin|read', {
      path: this.options.path,
      timeout: options?.timeout ?? this.options.timeout,
      size: options?.size ?? this.size,
    });
  }

  async readBinary(options?: ReadOptions): Promise<Uint8Array> {
    if (!this.isOpen) {
      throw new Error('Port is not open');
    }
    const result = await invoke<number[]>('plugin:serialplugin|read_binary', {
      path: this.options.path,
      timeout: options?.timeout ?? this.options.timeout,
      size: options?.size ?? this.size,
    });
    return new Uint8Array(result);
  }

  async setBaudRate(value: number | BaudRate): Promise<void> {
    return invoke<void>('plugin:serialplugin|set_baud_rate', {
      path: this.options.path,
      baudRate: value,
    });
  }

  async setDataBits(value: DataBits): Promise<void> {
    return invoke<void>('plugin:serialplugin|set_data_bits', {
      path: this.options.path,
      dataBits: value,
    });
  }

  async setFlowControl(value: FlowControl): Promise<void> {
    return invoke<void>('plugin:serialplugin|set_flow_control', {
      path: this.options.path,
      flowControl: value,
    });
  }

  async setParity(value: Parity): Promise<void> {
    return invoke<void>('plugin:serialplugin|set_parity', {
      path: this.options.path,
      parity: value,
    });
  }

  async setStopBits(value: StopBits): Promise<void> {
    return invoke<void>('plugin:serialplugin|set_stop_bits', {
      path: this.options.path,
      stopBits: value,
    });
  }

  async setTimeout(value: number): Promise<void> {
    return invoke<void>('plugin:serialplugin|set_timeout', {
      path: this.options.path,
      timeout: value,
    });
  }

  async writeRequestToSend(level: boolean): Promise<void> {
    return invoke<void>('plugin:serialplugin|write_request_to_send', {
      path: this.options.path,
      level,
    });
  }

  async writeDataTerminalReady(level: boolean): Promise<void> {
    return invoke<void>('plugin:serialplugin|write_data_terminal_ready', {
      path: this.options.path,
      level,
    });
  }

  async readClearToSend(): Promise<boolean> {
    return invoke<boolean>('plugin:serialplugin|read_clear_to_send', {
      path: this.options.path,
    });
  }

  async readDataSetReady(): Promise<boolean> {
    return invoke<boolean>('plugin:serialplugin|read_data_set_ready', {
      path: this.options.path,
    });
  }

  async readRingIndicator(): Promise<boolean> {
    return invoke<boolean>('plugin:serialplugin|read_ring_indicator', {
      path: this.options.path,
    });
  }

  async readCarrierDetect(): Promise<boolean> {
    return invoke<boolean>('plugin:serialplugin|read_carrier_detect', {
      path: this.options.path,
    });
  }

  async bytesToRead(): Promise<number> {
    return invoke<number>('plugin:serialplugin|bytes_to_read', {
      path: this.options.path,
    });
  }

  async bytesToWrite(): Promise<number> {
    return invoke<number>('plugin:serialplugin|bytes_to_write', {
      path: this.options.path,
    });
  }

  async clearBuffer(buffer: ClearBuffer): Promise<void> {
    return invoke<void>('plugin:serialplugin|clear_buffer', {
      path: this.options.path,
      bufferType: buffer,
    });
  }

  /** Write and read until terminators, idle silence, or timeout (native `exchange`). */
  async exchange(value: string, options?: ExchangeOptions): Promise<ExchangeResponse> {
    if (!this.isOpen) {
      throw new Error('Port is not open');
    }
    return invoke<ExchangeResponse>('plugin:serialplugin|exchange', {
      path: this.options.path,
      value,
      options: options ?? {},
    });
  }

  /** Binary write + read-until (e.g. CMGS PDU + Ctrl+Z). */
  async exchangeBinary(value: Uint8Array, options?: ExchangeOptions): Promise<ExchangeResponse> {
    if (!this.isOpen) {
      throw new Error('Port is not open');
    }
    return invoke<ExchangeResponse>('plugin:serialplugin|exchange_binary', {
      path: this.options.path,
      value: Array.from(value),
      options: options ?? {},
    });
  }

  /** Enter GSM 07.10 CMUX mode on this physical port. */
  async enableMux(options?: {
    command?: string;
    timeoutMs?: number;
  }): Promise<void> {
    if (!this.isOpen) {
      throw new Error('Port is not open');
    }
    await invoke<void>('plugin:serialplugin|enable_mux', {
      path: this.options.path,
      options: options ?? {},
    });
  }

  /** Open a virtual AT/data channel after CMUX is enabled. Returns a new `SerialPort`. */
  async openMuxChannel(dlci: number): Promise<SerialPort> {
    if (!this.isOpen) {
      throw new Error('Port is not open');
    }
    const virtualPath = await invoke<string>('plugin:serialplugin|open_mux_channel', {
      path: this.options.path,
      dlci,
    });
    return new SerialPort({
      path: virtualPath,
      baudRate: this.options.baudRate ?? 9600,
      dataBits: this.options.dataBits,
      flowControl: this.options.flowControl,
      parity: this.options.parity,
      stopBits: this.options.stopBits,
      timeout: this.options.timeout,
      size: this.options.size,
      serialDataFlushIntervalMs: this.options.serialDataFlushIntervalMs,
    });
  }

  /** Tear down CMUX and close all virtual channels on this physical port. */
  async disableMux(): Promise<void> {
    if (!this.isOpen) {
      return;
    }
    await invoke<void>('plugin:serialplugin|disable_mux', {
      path: this.options.path,
    });
  }

  /** Cancel an in-flight exchange / AT job on this port. */
  async cancelExchange(): Promise<void> {
    if (!this.isOpen) {
      return;
    }
    await invoke<void>('plugin:serialplugin|cancel_exchange', {
      path: this.options.path,
    });
  }

  /** Alias for [`cancelExchange`] — cancels native AT transaction queue. */
  async cancelAt(): Promise<void> {
    return this.cancelExchange();
  }

  /** Configure AT session defaults (native queue). Call after open if not using constructor `atSession`. */
  async configureAtSession(session: AtSessionOptions): Promise<void> {
    if (!this.isOpen) {
      throw new Error('Port is not open');
    }
    await invoke<void>('plugin:serialplugin|configure_at_session', {
      path: this.options.path,
      session,
    });
  }

  /** Send one AT command through the native FIFO queue. */
  async sendAt(command: string, options?: AtCommandOptions): Promise<AtCommandResult> {
    if (!this.isOpen) {
      throw new Error('Port is not open');
    }
    const result = await invoke<AtCommandResult>('plugin:serialplugin|at', {
      path: this.options.path,
      command,
      options: options ?? null,
    });
    return {
      ...result,
      raw: new Uint8Array(result.raw as unknown as number[]),
    };
  }

  /** Multi-phase AT flow (e.g. CMGS) as one atomic native queue job. */
  async sendAtPhases(phases: AtPhase[]): Promise<AtCommandResult[]> {
    if (!this.isOpen) {
      throw new Error('Port is not open');
    }
    const results = await invoke<AtCommandResult[]>('plugin:serialplugin|at_phases', {
      path: this.options.path,
      phases,
    });
    return results.map((r) => ({
      ...r,
      raw: new Uint8Array(r.raw as unknown as number[]),
    }));
  }

  /** Built-in CMGS recipe: `AT+CMGS=n` → `>` → PDU + Ctrl+Z. */
  async sendSmsPdu(
    length: number,
    pdu: Uint8Array,
    options?: SendSmsPduOptions,
  ): Promise<AtCommandResult[]> {
    if (!this.isOpen) {
      throw new Error('Port is not open');
    }
    const results = await invoke<AtCommandResult[]>('plugin:serialplugin|send_sms_pdu', {
      path: this.options.path,
      length,
      pdu: Array.from(pdu),
      options: options ?? null,
    });
    return results.map((r) => ({
      ...r,
      raw: new Uint8Array(r.raw as unknown as number[]),
    }));
  }

  async setBreak(): Promise<void> {
    return invoke<void>('plugin:serialplugin|set_break', {
      path: this.options.path,
    });
  }

  async clearBreak(): Promise<void> {
    return invoke<void>('plugin:serialplugin|clear_break', {
      path: this.options.path,
    });
  }

  async write(value: string): Promise<number> {
    if (!this.isOpen) {
      throw new Error('Port is not open');
    }
    return invoke<number>('plugin:serialplugin|write', {
      value,
      path: this.options.path,
    });
  }

  async writeBinary(value: Uint8Array | number[]): Promise<number> {
    if (!this.isOpen) {
      throw new Error('Port is not open');
    }
    if (!(value instanceof Uint8Array || Array.isArray(value))) {
      throw new Error('value argument type error: expected Uint8Array or number[]');
    }
    return invoke<number>('plugin:serialplugin|write_binary', {
      value: Array.from(value),
      path: this.options.path,
    });
  }
}
