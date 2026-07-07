/** Default read/write timeout (ms) when opening a port on desktop. */
export const DEFAULT_SERIAL_TIMEOUT_MS = 1000;

export interface PortInfo {
  path: 'Unknown' | string;
  manufacturer: 'Unknown' | string;
  pid: 'Unknown' | string;
  product: 'Unknown' | string;
  serial_number: 'Unknown' | string;
  type: 'PCI' | string;
  vid: 'Unknown' | string;
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
  /** Desktop: batch coalescing window (ms). Android: coalescing hint where supported. */
  timeout?: number;
  /** Max bytes per read chunk. */
  size?: number;
  /**
   * Desktop: preferred batch flush interval (falls back to `timeout`).
   * Android: `BufferedEmitter` flush interval (10–2000 ms).
   */
  serialDataFlushIntervalMs?: number;
  /** Default `true` — stream-decode with TextDecoder. */
  decode?: boolean;
}

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

export type SerialPortConfig = SerialportOptions;

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
  | { cmeError: { code?: number | null } }
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
