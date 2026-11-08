// Setup file for vitest (jsdom)

const { MockChannel } = vi.hoisted(() => {
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
  return { MockChannel };
});

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
  Channel: MockChannel,
}));

Object.defineProperty(window, '__TAURI_EVENT_PLUGIN_INTERNALS__', {
  value: {
    unregisterListener: vi.fn(),
    registerListener: vi.fn(),
  },
  writable: true,
});

Object.defineProperty(window, '__TAURI__', {
  value: {
    invoke: vi.fn(),
  },
  writable: true,
});

import { resetCapabilitiesCacheForTests } from '../guest-js/serial-port';

beforeEach(() => {
  vi.clearAllMocks();
  MockChannel.reset();
  resetCapabilitiesCacheForTests();
  vi.spyOn(console, 'warn').mockImplementation(() => {});
  vi.spyOn(console, 'error').mockImplementation(() => {});
});

afterEach(() => {
  vi.restoreAllMocks();
});

export { MockChannel };
