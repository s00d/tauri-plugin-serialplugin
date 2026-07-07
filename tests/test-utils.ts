import { SerialPort, DataBits, FlowControl, Parity, StopBits } from '../guest-js';
import type { SerialportOptions } from '../guest-js';
import { invoke } from '@tauri-apps/api/core';

export type MockInvoke = jest.Mock<Promise<any>, [string, any?]>;

export const setupTestMocks = () => {
  const mockInvoke = invoke as MockInvoke;
  jest.clearAllMocks();

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
  jest.clearAllTimers();
});

afterAll(() => {
  jest.clearAllTimers();
});
