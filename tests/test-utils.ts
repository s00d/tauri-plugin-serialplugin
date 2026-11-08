import type { Mock } from 'vitest';
import { SerialPort, DataBits, FlowControl, Parity, StopBits } from '../guest-js';
import type { SerialportOptions } from '../guest-js';
import { invoke } from '@tauri-apps/api/core';

export type MockInvoke = Mock<(cmd: string, args?: any) => Promise<any>>;

/** Bytes as the Rust side returns them (`Vec<u8>` → number[]). */
export const wire = (text: string): number[] => Array.from(new TextEncoder().encode(text));

export const setupTestMocks = () => {
  const mockInvoke = invoke as MockInvoke;
  vi.clearAllMocks();

  return {
    mockInvoke,
  };
};

export const createTestSerialPort = (overrides?: Partial<SerialportOptions>) => {
  return new SerialPort({
    path: '/dev/tty.usbserial',
    baudRate: 9600,
    dataBits: DataBits.Eight,
    flowControl: FlowControl.None,
    parity: Parity.None,
    stopBits: StopBits.One,
    timeout: 1000,
    ...overrides,
  });
};

afterEach(() => {
  vi.clearAllTimers();
});

afterAll(() => {
  vi.clearAllTimers();
});
