// Setup file for Jest tests
// This file runs before each test file

// Mock Tauri API modules
jest.mock('@tauri-apps/api/core', () => ({
  invoke: jest.fn(),
}));

jest.mock('@tauri-apps/api/event', () => ({
  listen: jest.fn(),
  once: jest.fn(),
}));

// Mock window.__TAURI_EVENT_PLUGIN_INTERNALS__ for Tauri API
Object.defineProperty(window, '__TAURI_EVENT_PLUGIN_INTERNALS__', {
  value: {
    unregisterListener: jest.fn(),
    registerListener: jest.fn(),
  },
  writable: true,
});

// Mock window.__TAURI__ for Tauri API
Object.defineProperty(window, '__TAURI__', {
  value: {
    invoke: jest.fn(),
  },
  writable: true,
});

// Mock TextDecoder and TextEncoder for encoding tests
global.TextDecoder = class TextDecoder {
  constructor(encoding?: string) {}
  decode(input?: Uint8Array | ArrayBuffer | null): string {
    return String.fromCharCode(...(input as Uint8Array));
  }
} as any;

global.TextEncoder = class TextEncoder {
  encode(input?: string): Uint8Array {
    return new Uint8Array(input?.split('').map(c => c.charCodeAt(0)) || []);
  }
} as any;

// Mock console methods to avoid noise in tests
const originalConsole = { ...console };
beforeEach(() => {
  jest.spyOn(console, 'warn').mockImplementation(() => {});
  jest.spyOn(console, 'error').mockImplementation(() => {});
});

afterEach(() => {
  jest.restoreAllMocks();
});