import { SerialPort } from '../guest-js';
import { MockChannel } from './setup';
import { setupTestMocks, createTestSerialPort } from './test-utils';

describe('SerialPort Auto-Reconnect', () => {
  let serialPort: SerialPort;
  let mockInvoke: ReturnType<typeof setupTestMocks>['mockInvoke'];
  let watchCallCount = 0;

  beforeEach(() => {
    jest.useFakeTimers();
    watchCallCount = 0;
    const mocks = setupTestMocks();
    mockInvoke = mocks.mockInvoke;
    serialPort = createTestSerialPort();
  });

  afterEach(async () => {
    jest.useRealTimers();
    await SerialPort.closeAll();
  });

  it('automatically reconnects and re-watch after disconnect', async () => {
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'plugin:serialplugin|open') return Promise.resolve('/dev/tty.usbserial');
      if (cmd === 'plugin:serialplugin|watch') {
        watchCallCount += 1;
        return Promise.resolve(watchCallCount);
      }
      if (cmd === 'plugin:serialplugin|unwatch') return Promise.resolve();
      return Promise.resolve();
    });

    await serialPort.open();
    serialPort.enableAutoReconnect({ interval: 100 });
    const onData = jest.fn();
    await serialPort.watch({ onData });

    MockChannel.lastInstance!.onmessage?.({
      kind: 'disconnect',
      path: '/dev/tty.usbserial',
      reason: 'lost',
    });

    await jest.runOnlyPendingTimersAsync();

    expect(serialPort.isOpen).toBe(true);
    expect(watchCallCount).toBe(2);
  });

  it('does not re-watch without enableAutoReconnect', async () => {
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'plugin:serialplugin|open') return Promise.resolve('/dev/tty.usbserial');
      if (cmd === 'plugin:serialplugin|watch') {
        watchCallCount += 1;
        return Promise.resolve(1);
      }
      if (cmd === 'plugin:serialplugin|unwatch') return Promise.resolve();
      return Promise.resolve();
    });

    await serialPort.open();
    await serialPort.watch({ onData: jest.fn() });

    const openCallsBeforeDisconnect = mockInvoke.mock.calls.filter(
      (call) => call[0] === 'plugin:serialplugin|open',
    ).length;

    MockChannel.lastInstance!.onmessage?.({
      kind: 'disconnect',
      path: '/dev/tty.usbserial',
      reason: 'lost',
    });

    await jest.runOnlyPendingTimersAsync();

    expect(serialPort.isOpen).toBe(false);
    expect(watchCallCount).toBe(1);
    const openCallsAfterDisconnect = mockInvoke.mock.calls.filter(
      (call) => call[0] === 'plugin:serialplugin|open',
    ).length;
    expect(openCallsAfterDisconnect).toBe(openCallsBeforeDisconnect);
  });

  it('getAutoReconnectInfo reflects configuration', async () => {
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'plugin:serialplugin|open') return Promise.resolve('/dev/tty.usbserial');
      return Promise.resolve();
    });
    await serialPort.open();
    serialPort.enableAutoReconnect({ interval: 250, maxAttempts: 3 });
    const info = serialPort.getAutoReconnectInfo();
    expect(info.enabled).toBe(true);
    expect(info.interval).toBe(250);
    expect(info.maxAttempts).toBe(3);
  });

  it('manualReconnect reopens and re-watch', async () => {
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'plugin:serialplugin|open') return Promise.resolve('/dev/tty.usbserial');
      if (cmd === 'plugin:serialplugin|watch') {
        watchCallCount += 1;
        return Promise.resolve(watchCallCount);
      }
      if (cmd === 'plugin:serialplugin|unwatch') return Promise.resolve();
      return Promise.resolve();
    });

    await serialPort.open();
    await serialPort.watch({ onData: jest.fn() });
    serialPort.isOpen = false;

    const ok = await serialPort.manualReconnect();
    expect(ok).toBe(true);
    expect(watchCallCount).toBe(2);
  });

  it('stops auto-reconnect after maxAttempts is exhausted', async () => {
    let openCount = 0;
    const onReconnect = jest.fn();

    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'plugin:serialplugin|open') {
        openCount += 1;
        if (openCount === 1) {
          return Promise.resolve('/dev/tty.usbserial');
        }
        return Promise.reject(new Error('reconnect failed'));
      }
      if (cmd === 'plugin:serialplugin|watch') {
        watchCallCount += 1;
        return Promise.resolve(watchCallCount);
      }
      if (cmd === 'plugin:serialplugin|unwatch') return Promise.resolve();
      return Promise.resolve();
    });

    await serialPort.open();
    serialPort.enableAutoReconnect({ interval: 100, maxAttempts: 2, onReconnect });
    await serialPort.watch({ onData: jest.fn() });

    MockChannel.lastInstance!.onmessage?.({
      kind: 'disconnect',
      path: '/dev/tty.usbserial',
      reason: 'lost',
    });

    await jest.runAllTimersAsync();

    expect(openCount).toBe(3);
    expect(serialPort.isOpen).toBe(false);
    expect(onReconnect).toHaveBeenCalledWith(false, 2);
  });

  it('disableAutoReconnect prevents reopen after disconnect', async () => {
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'plugin:serialplugin|open') return Promise.resolve('/dev/tty.usbserial');
      if (cmd === 'plugin:serialplugin|watch') {
        watchCallCount += 1;
        return Promise.resolve(watchCallCount);
      }
      if (cmd === 'plugin:serialplugin|unwatch') return Promise.resolve();
      return Promise.resolve();
    });

    await serialPort.open();
    serialPort.enableAutoReconnect({ interval: 100 });
    await serialPort.watch({ onData: jest.fn() });
    serialPort.disableAutoReconnect();

    const openCallsBeforeDisconnect = mockInvoke.mock.calls.filter(
      (call) => call[0] === 'plugin:serialplugin|open',
    ).length;

    MockChannel.lastInstance!.onmessage?.({
      kind: 'disconnect',
      path: '/dev/tty.usbserial',
      reason: 'lost',
    });

    await jest.runOnlyPendingTimersAsync();

    const openCallsAfterDisconnect = mockInvoke.mock.calls.filter(
      (call) => call[0] === 'plugin:serialplugin|open',
    ).length;
    expect(openCallsAfterDisconnect).toBe(openCallsBeforeDisconnect);
    expect(serialPort.isOpen).toBe(false);
    expect(serialPort.getAutoReconnectInfo().enabled).toBe(false);
  });
});
