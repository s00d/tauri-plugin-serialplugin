import { SerialPort, DataBits, FlowControl, Parity, StopBits } from '../guest-js';
import { invoke } from "@tauri-apps/api/core";
import { listen, once } from '@tauri-apps/api/event';

// Types for mocks
export type MockInvoke = jest.Mock<Promise<any>, [string, any?]>;
export type MockListen = jest.Mock<Promise<() => void>, [string, (event: any) => void]>;
export type MockOnce = jest.Mock<Promise<void>, [string, (event: any) => void]>;

// Test setup utilities
export const setupTestMocks = () => {
  const mockInvoke = invoke as MockInvoke;
  const mockListen = listen as MockListen;
  const mockOnce = once as unknown as MockOnce;
  
  const mockUnlisten = jest.fn().mockImplementation(() => {
    return Promise.resolve();
  });
  
  mockListen.mockResolvedValue(mockUnlisten);
  mockOnce.mockResolvedValue(undefined);
  jest.clearAllMocks();

  return {
    mockInvoke,
    mockListen,
    mockOnce,
    mockUnlisten
  };
};

export const createTestSerialPort = () => {
  return new SerialPort({
    path: '/dev/tty.usbserial',
    baudRate: 9600,
    dataBits: DataBits.Eight,
    flowControl: FlowControl.None,
    parity: Parity.None,
    stopBits: StopBits.One,
    timeout: 1000
  });
};

// Global test cleanup
afterEach(() => {
  jest.clearAllTimers();
});

afterAll(() => {
  jest.clearAllTimers();
}); 
 