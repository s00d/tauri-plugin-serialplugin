export type WatchLogKind = 'data' | 'error' | 'disconnect' | 'info';

export interface WatchLogEntry {
  id: number;
  time: string;
  kind: WatchLogKind;
  message: string;
}

export type TerminalLineKind = 'rx' | 'tx' | 'sys' | 'urc' | 'err' | 'at' | 'disconnect';

export interface TerminalLine {
  id: number;
  time: string;
  kind: TerminalLineKind;
  text: string;
}

export type LineEnding = 'lf' | 'cr' | 'crlf' | 'none';

export interface PortListEntry {
  path: string;
  meta: string;
  source: 'available' | 'managed' | 'manual';
}

export type SendMode = 'text' | 'at' | 'hex';
