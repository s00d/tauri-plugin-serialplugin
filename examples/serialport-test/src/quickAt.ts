import type { LineEnding, SendMode } from './types';

export type QuickAtSend = {
  text: string;
  mode: SendMode;
  lineEnding: LineEnding;
  localEcho: boolean;
};

/** Guard for one-click AT buttons (connected and not mid-AT). */
export function canSendQuickAt(connected: boolean, atBusy?: boolean): boolean {
  return Boolean(connected) && !atBusy;
}

/** Payload emitted to the session for a quick AT probe. */
export function buildQuickAtSend(
  cmd: string,
  lineEnding: LineEnding,
  localEcho: boolean,
): QuickAtSend {
  return {
    text: cmd,
    mode: 'at',
    lineEnding,
    localEcho,
  };
}
