/** Default read/write timeout (ms) when opening a port on desktop. */
export const DEFAULT_SERIAL_TIMEOUT_MS = 1000;

/** Port metadata; the port path is the key of the record it comes in. */
export interface PortInfo {
  /** @deprecated Never sent by the native side; use the record key. */
  path?: string;
  manufacturer: 'Unknown' | string;
  pid: 'Unknown' | string;
  product: 'Unknown' | string;
  serial_number: 'Unknown' | string;
  type: 'PCI' | string;
  vid: 'Unknown' | string;
}

/** A `SerialPort.scanBlePorts()` result; the `ble://…` path is the record key. */
export interface BlePortInfo extends PortInfo {
  /** Signal strength in dBm as a decimal string; empty when the platform did not report it. */
  rssi: string;
}

export interface Capabilities {
  transport: 'desktop' | 'mobile';
  platform: string;
  version: string;
}

export type SerialEvent =
  | { kind: 'data'; path: string; data: number[]; size: number }
  | { kind: 'urc'; path: string; line: string }
  | { kind: 'disconnect'; path: string; reason: string }
  | { kind: 'error'; path: string; message: string };

export interface WatchOptions {
  /** Batch coalescing window (ms). Default ~20 ms; not the port's read timeout. */
  timeout?: number;
  /** Max bytes per read chunk; a batch is also flushed once this many bytes are buffered. */
  size?: number;
  /** Batch flush interval (ms); takes precedence over `timeout`. All platforms. */
  serialDataFlushIntervalMs?: number;
  /** Default `true` — stream-decode with TextDecoder. */
  decode?: boolean;
  /**
   * When `true`, native hub line-routes idle RX so AT URC lines become `onUrc` events
   * (and are removed from `onData`). Default: `true` if `onUrc` is provided, else `false`
   * (raw byte stream — required for binary / NMEA). Ignored on CMUX channels
   * (`openMuxChannel()`), as is `size`.
   */
  routeUrc?: boolean;
  /**
   * Frame RX natively (Node `@serialport/parser-*` semantics): each complete frame
   * becomes exactly one `onData` call and the batch window (`timeout`,
   * `serialDataFlushIntervalMs`) no longer applies. With `decode` (default) each frame
   * is decoded on its own. An invalid config rejects `watch()` before the watch starts.
   *
   * With `routeUrc` / `onUrc`, URC lines are split off first and only the other lines
   * reach the parser. On `unwatch()`, `close()` or a disconnect, the parser's buffered
   * partial frame is delivered as a last `onData` call (before `onDisconnect`).
   * Auto-reconnect restores the watch with the same parser.
   *
   * @example
   * await port.watch({ onData: (line) => console.log(line) }, { parser: { type: 'readline', delimiter: '\r\n' } });
   */
  parser?: ParserOptions;
}

/** Bytes as a UTF-8 string, a byte array or a `Uint8Array`. */
export type ByteSeq = string | number[] | Uint8Array;

/**
 * Native parser selection for `watch({ parser })`. `maxBufferSize` (default 65536) caps
 * bytes buffered without a complete frame; see each variant for what happens at the cap.
 * Unknown keys make `watch` fail (a typo would otherwise fall back to a default).
 */
export type ParserOptions =
  /** Split on a byte sequence (`@serialport/parser-delimiter`). Full buffer is emitted as a frame. */
  | { type: 'delimiter'; delimiter: ByteSeq; includeDelimiter?: boolean; maxBufferSize?: number }
  /** Split on `delimiter`, default `"\n"` (`@serialport/parser-readline`). */
  | {
      type: 'readline';
      delimiter?: ByteSeq;
      includeDelimiter?: boolean;
      maxBufferSize?: number;
      /** Node's string-rendering option: accepted and ignored (frames are decoded by `watch`). */
      encoding?: string;
    }
  /** Fixed-size frames of `length` bytes (`@serialport/parser-byte-length`). */
  | { type: 'byteLength'; length: number }
  /**
   * A frame ends after `intervalMs` (alias `interval`) of silence (`@serialport/parser-inter-byte-timeout`).
   * Give exactly one of `intervalMs` / `interval`.
   */
  | { type: 'interByteTimeout'; intervalMs: number; interval?: never; maxBufferSize?: number }
  | { type: 'interByteTimeout'; interval: number; intervalMs?: never; maxBufferSize?: number }
  /**
   * Delimiter + little-endian length field (`@serialport/parser-packet-length`).
   * Defaults: delimiter 0xaa, delimiterBytes 1, packetOverhead 2, lengthBytes 1,
   * lengthOffset 1, maxLen 0xff, maxBufferSize 65536 (a packet that cannot complete
   * within it is emitted as a frame).
   */
  | {
      type: 'packetLength';
      delimiter?: number | number[];
      delimiterBytes?: number;
      packetOverhead?: number;
      lengthBytes?: number;
      lengthOffset?: number;
      maxLen?: number;
      maxBufferSize?: number;
    }
  /** Drop everything until `delimiter`, then pass data through (`@serialport/parser-ready`). */
  | { type: 'ready'; delimiter: ByteSeq }
  /** Split UTF-8 text on a regular expression (Rust `regex-lite` syntax, not a JS `RegExp`). */
  | {
      type: 'regex';
      regex: string;
      maxBufferSize?: number;
      /** Node's string-rendering option: accepted and ignored. */
      encoding?: string;
    }
  /** Frames between a start and an end marker (`@serialport/parser-start-end`). */
  | {
      type: 'startEnd';
      startDelimiter: ByteSeq;
      endDelimiter: ByteSeq;
      includeStartDelimiter?: boolean;
      includeEndDelimiter?: boolean;
      maxBufferSize?: number;
    }
  /** SLIP (RFC 1055) decoder (`@serialport/parser-slip-encoder` `SlipDecoder`); bytes default to RFC values. */
  | {
      type: 'slip';
      start?: number;
      esc?: number;
      end?: number;
      escStart?: number;
      escEnd?: number;
      escEsc?: number;
    }
  /** ccTalk frames, length in byte 1 (`@serialport/parser-cctalk`). A partial frame is dropped after `maxDelayMs` (default 50, 0 = never). */
  | { type: 'ccTalk'; maxDelayMs?: number };

export interface WatchHandlers {
  onData: (data: string | Uint8Array) => void;
  /** Unsolicited AT line (`+CREG:` etc.) when watch stays active during AT. */
  onUrc?: (line: string) => void;
  onDisconnect?: (reason: string) => void;
  onError?: (message: string) => void;
}

export interface WatchHandle {
  channelId: number;
  unwatch(): Promise<void>;
}

export interface AvailablePortsOptions {
  singlePortPerDevice?: boolean;
}

export interface WatchPortsOptions {
  /** Same as `available_ports({ singlePortPerDevice })`. Default: false */
  singlePortPerDevice?: boolean;
  /** Poll interval (ms). Desktop default 2000; Android uses USB events + poll fallback. */
  pollIntervalMs?: number;
}

export type PortListEvent =
  | { kind: 'snapshot'; ports: Record<string, PortInfo> }
  | { kind: 'added'; path: string; info: PortInfo }
  | { kind: 'removed'; path: string };

export interface WatchPortsHandlers {
  onSnapshot?: (ports: Record<string, PortInfo>) => void;
  onAdded?: (path: string, info: PortInfo) => void;
  onRemoved?: (path: string) => void;
}

export interface SerialportOptions {
  /**
   * DTR level on open. `false` keeps Arduino-style boards from resetting when the
   * port opens (Linux still pulses DTR briefly). Default: platform default
   * (desktop: unchanged, Android: asserted).
   */
  dtr?: boolean;
  /** RTS level on open. Default: platform default (desktop: unchanged, Android: asserted). */
  rts?: boolean;
  path: string;
  baudRate: number;
  encoding?: string;
  dataBits?: DataBits;
  flowControl?: FlowControl;
  parity?: Parity;
  stopBits?: StopBits;
  timeout?: number;
  size?: number;
  serialDataFlushIntervalMs?: number;
  /** Default AT session options applied on `open()` via native `configureAtSession`. */
  atSession?: AtSessionOptions;
}

export interface Options {
  /**
   * DTR level on open. `false` keeps Arduino-style boards from resetting when the
   * port opens (Linux still pulses DTR briefly). Default: platform default
   * (desktop: unchanged, Android: asserted).
   */
  dtr?: boolean;
  /** RTS level on open. Default: platform default (desktop: unchanged, Android: asserted). */
  rts?: boolean;
  path?: string;
  baudRate?: number;
  dataBits: DataBits;
  flowControl: FlowControl;
  parity: Parity;
  stopBits: StopBits;
  size?: number;
  timeout: number;
  serialDataFlushIntervalMs?: number;
}

export interface ReadOptions {
  timeout?: number;
  size?: number;
}

export enum DataBits {
  Five = 'Five',
  Six = 'Six',
  Seven = 'Seven',
  Eight = 'Eight',
}

export enum FlowControl {
  None = 'None',
  Software = 'Software',
  Hardware = 'Hardware',
}

export enum Parity {
  None = 'None',
  Odd = 'Odd',
  Even = 'Even',
}

export enum StopBits {
  One = 'One',
  Two = 'Two',
}

export enum ClearBuffer {
  Input = 'Input',
  Output = 'Output',
  All = 'All',
}

export interface AutoReconnectOptions {
  interval?: number;
  maxAttempts?: number | null;
  onReconnect?: (success: boolean, attempt: number) => void;
}

export interface AutoReconnectInfo {
  enabled: boolean;
  interval: number;
  maxAttempts: number | null;
  currentAttempts: number;
  hasCallback: boolean;
}

export type BaudRate =
  | 110
  | 300
  | 600
  | 1200
  | 2400
  | 4800
  | 9600
  | 14400
  | 19200
  | 38400
  | 57600
  | 115200
  | 230400
  | 460800
  | 921600;

export type RxPrepareMode = 'drain' | 'purge' | 'none';
export type AtResultFormat = 'verbose' | 'numeric';
export type ExchangeCompletionMode = 'atFinalLine' | 'atIntermediate' | 'substring';
export type AtParseStatus = 'ok' | 'error' | 'cme' | 'cms' | 'unknown';

export type ExchangeMatch =
  | 'ok'
  | 'error'
  | 'idle'
  | 'noCarrier'
  | 'busy'
  | 'noAnswer'
  | 'noDialtone'
  | 'sendOk'
  | 'sendFail'
  /**
   * Verbose texts (`AT+CMEE=2`) are mapped to their numeric `code` (`"SIM busy"` → 14);
   * `code` is `null` only for text the parser does not know.
   */
  | { cmeError: { code?: number | null } }
  /** Same shape as `cmeError`, for `+CMS ERROR`. */
  | { cmsError: { code?: number | null } }
  | { intermediate: { line: string } }
  | { substring: { term: string } };

/** Options passed to native `exchange` (write + read-until). */
export interface ExchangeOptions {
  timeoutMs?: number;
  maxBytes?: number;
  terminators?: string[];
  idleMs?: number;
  rxPrepare?: RxPrepareMode;
  drainIdleMs?: number;
  drainMaxMs?: number;
  completionMode?: ExchangeCompletionMode;
  resultFormat?: AtResultFormat;
  command?: string;
  solicitedPrefixes?: string[];
}

/** Structured native exchange response. */
export interface ExchangeResponse {
  raw: Uint8Array;
  matched: ExchangeMatch;
  lines: string[];
  status: AtParseStatus;
  solicitedBody: string[];
  urcLines: string[];
}

export interface AtSessionOptions {
  defaultTimeoutMs?: number;
  defaultTerminators?: string[];
  defaultIdleMs?: number;
  defaultRxPrepare?: RxPrepareMode;
  defaultSolicitedPrefixes?: string[];
  /** Stop processing queue on first error. Default: true */
  stopOnError?: boolean;
  /** Reject promise when native status is not `ok`. Default: false */
  expectOk?: boolean;
  /** Append `\r` when command has no line ending. Default: true */
  appendCr?: boolean;
  /** Verbose (`ATV1`) vs numeric (`ATV0`) result lines. Default: verbose */
  resultFormat?: AtResultFormat;
}

export interface AtPhase {
  write: string | Uint8Array;
  completionMode?: ExchangeCompletionMode;
  resultFormat?: AtResultFormat;
  timeoutMs?: number;
  command?: string;
  rxPrepare?: RxPrepareMode;
}

export interface SendSmsPduOptions {
  timeoutMs?: number;
  resultFormat?: AtResultFormat;
}

export interface AtCommandOptions {
  timeoutMs?: number;
  terminators?: string[];
  idleMs?: number;
  rxPrepare?: RxPrepareMode;
  completionMode?: ExchangeCompletionMode;
  resultFormat?: AtResultFormat;
  solicitedPrefixes?: string[];
  appendCr?: boolean;
}

export interface AtCommandResult {
  command: string;
  response: string;
  raw: Uint8Array;
  status: AtParseStatus;
  lines: string[];
  solicitedBody: string[];
  urcLines: string[];
  matched: ExchangeMatch;
  timedOut: boolean;
}
