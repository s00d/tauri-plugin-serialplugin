import { Channel } from '@tauri-apps/api/core';
import { SerialPort } from '../guest-js';
import { MockChannel } from './setup';
import { createTestSerialPort, setupTestMocks } from './test-utils';

describe('SerialPort watch lifecycle', () => {
  beforeEach(() => {
    setupTestMocks();
  });

  it('invokes watch with channel and returns handle', async () => {
    const { mockInvoke } = setupTestMocks();
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'plugin:serialplugin|open') return Promise.resolve();
      if (cmd === 'plugin:serialplugin|watch') return Promise.resolve(7);
      if (cmd === 'plugin:serialplugin|unwatch') return Promise.resolve();
      return Promise.resolve();
    });

    const port = createTestSerialPort();
    await port.open();

    const onData = jest.fn();
    const handle = await port.watch({ onData });

    expect(handle.channelId).toBe(7);
    expect(mockInvoke).toHaveBeenCalledWith(
      'plugin:serialplugin|watch',
      expect.objectContaining({
        path: '/dev/tty.usbserial',
        channel: expect.any(Channel),
      }),
    );

    await handle.unwatch();
    expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|unwatch', {
      channelId: 7,
    });
  });

  it('decodes data events on the channel', async () => {
    const { mockInvoke } = setupTestMocks();
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'plugin:serialplugin|open') return Promise.resolve();
      if (cmd === 'plugin:serialplugin|watch') return Promise.resolve(1);
      return Promise.resolve();
    });

    const port = createTestSerialPort();
    await port.open();
    const onData = jest.fn();
    await port.watch({ onData });

    const channel = MockChannel.lastInstance;
    expect(channel).toBeTruthy();
    channel!.onmessage?.({
      kind: 'data',
      path: '/dev/tty.usbserial',
      data: [72, 105],
      size: 2,
    });

    expect(onData).toHaveBeenCalledWith('Hi');
  });

  it('calls onError without closing the port', async () => {
    const { mockInvoke } = setupTestMocks();
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'plugin:serialplugin|open') return Promise.resolve();
      if (cmd === 'plugin:serialplugin|watch') return Promise.resolve(1);
      return Promise.resolve();
    });

    const port = createTestSerialPort();
    await port.open();
    const onError = jest.fn();
    await port.watch({ onData: jest.fn(), onError });

    MockChannel.lastInstance!.onmessage?.({
      kind: 'error',
      path: '/dev/tty.usbserial',
      message: 'transient glitch',
    });

    expect(onError).toHaveBeenCalledWith('transient glitch');
    expect(port.isOpen).toBe(true);
  });

  it('calls onDisconnect and marks port closed', async () => {
    const { mockInvoke } = setupTestMocks();
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'plugin:serialplugin|open') return Promise.resolve();
      if (cmd === 'plugin:serialplugin|watch') return Promise.resolve(1);
      return Promise.resolve();
    });

    const port = createTestSerialPort();
    await port.open();
    const onDisconnect = jest.fn();
    await port.watch({ onData: jest.fn(), onDisconnect });

    MockChannel.lastInstance!.onmessage?.({
      kind: 'disconnect',
      path: '/dev/tty.usbserial',
      reason: 'USB detached',
    });

    expect(onDisconnect).toHaveBeenCalledWith('USB detached');
    expect(port.isOpen).toBe(false);
  });

  it('rejects duplicate watch on same instance', async () => {
    const { mockInvoke } = setupTestMocks();
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'plugin:serialplugin|open') return Promise.resolve();
      if (cmd === 'plugin:serialplugin|watch') return Promise.resolve(1);
      return Promise.resolve();
    });

    const port = createTestSerialPort();
    await port.open();
    await port.watch({ onData: jest.fn() });
    await expect(port.watch({ onData: jest.fn() })).rejects.toThrow(
      'A watch is already active on this port instance',
    );
  });

  it('ignores events for other paths', async () => {
    const { mockInvoke } = setupTestMocks();
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'plugin:serialplugin|open') return Promise.resolve();
      if (cmd === 'plugin:serialplugin|watch') return Promise.resolve(1);
      return Promise.resolve();
    });

    const port = createTestSerialPort();
    await port.open();
    const onData = jest.fn();
    await port.watch({ onData });

    MockChannel.lastInstance!.onmessage?.({
      kind: 'data',
      path: '/dev/other',
      data: [65],
      size: 1,
    });

    expect(onData).not.toHaveBeenCalled();
  });

  it('unwatch on close', async () => {
    const { mockInvoke } = setupTestMocks();
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'plugin:serialplugin|open') return Promise.resolve();
      if (cmd === 'plugin:serialplugin|watch') return Promise.resolve(3);
      if (cmd === 'plugin:serialplugin|unwatch') return Promise.resolve();
      if (cmd === 'plugin:serialplugin|close') return Promise.resolve();
      if (cmd === 'plugin:serialplugin|cancel_read') return Promise.resolve();
      return Promise.resolve();
    });

    const port = createTestSerialPort();
    await port.open();
    await port.watch({ onData: jest.fn() });
    await port.close();

    expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|unwatch', {
      channelId: 3,
    });
  });
});

describe('getCapabilities', () => {
  it('caches capabilities invoke', async () => {
    const { mockInvoke } = setupTestMocks();
    mockInvoke.mockResolvedValue({
      transport: 'desktop',
      platform: 'macos',
      version: '3.0.0',
    });

    const a = await SerialPort.getCapabilities();
    const b = await SerialPort.getCapabilities();

    expect(a.platform).toBe('macos');
    expect(b.version).toBe('3.0.0');
    expect(mockInvoke).toHaveBeenCalledTimes(1);
  });
});
