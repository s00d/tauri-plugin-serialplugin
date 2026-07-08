// Setup file for Jest tests

class MockChannel<T> {
  static lastInstance: MockChannel<unknown> | null = null;
  onmessage: ((message: T) => void) | null = null;
  id = 1;

  constructor() {
    MockChannel.lastInstance = this as MockChannel<unknown>;
  }

  static reset() {
    MockChannel.lastInstance = null;
  }
}

jest.mock('@tauri-apps/api/core', () => ({
  invoke: jest.fn(),
  Channel: MockChannel,
}));

Object.defineProperty(window, '__TAURI_EVENT_PLUGIN_INTERNALS__', {
  value: {
    unregisterListener: jest.fn(),
    registerListener: jest.fn(),
  },
  writable: true,
});

Object.defineProperty(window, '__TAURI__', {
  value: {
    invoke: jest.fn(),
  },
  writable: true,
});

global.TextDecoder = class TextDecoder {
  constructor(_encoding?: string) {}
  decode(input?: Uint8Array | ArrayBuffer | null): string {
    return String.fromCharCode(...(input as Uint8Array));
  }
} as typeof TextDecoder;

global.TextEncoder = class TextEncoder {
  encode(input?: string): Uint8Array {
    return new Uint8Array(input?.split('').map((c) => c.charCodeAt(0)) || []);
  }
} as typeof TextEncoder;

import { resetCapabilitiesCacheForTests } from '../guest-js/serial-port';

beforeEach(() => {
  jest.clearAllMocks();
  MockChannel.reset();
  resetCapabilitiesCacheForTests();
  jest.spyOn(console, 'warn').mockImplementation(() => {});
  jest.spyOn(console, 'error').mockImplementation(() => {});
});

afterEach(() => {
  jest.restoreAllMocks();
});

export { MockChannel };
