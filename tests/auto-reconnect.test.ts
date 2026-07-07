import { SerialPort } from '../guest-js';
import { MockChannel } from './setup';
import { setupTestMocks, createTestSerialPort } from './test-utils';

describe('SerialPort Auto-Reconnect', () => {
  let serialPort: SerialPort;
  let mockInvoke: ReturnType<typeof setupTestMocks>['mockInvoke'];
  let watchCallCount = 0;

  beforeEach(() => {
    watchCallCount = 0;
    const mocks = setupTestMocks();
    mockInvoke = mocks.mockInvoke;
    serialPort = createTestSerialPort();
  });

  afterEach(async () => {
    await SerialPort.closeAll();
  });

  it('should automatically reconnect and re-watch after a disconnect event', async () => {
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'plugin:serialplugin|open') return Promise.resolve();
      if (cmd === 'plugin:serialplugin|watch') {
        watchCallCount += 1;
        return Promise.resolve(watchCallCount);
      }
      if (cmd === 'plugin:serialplugin|unwatch') return Promise.resolve();
      return Promise.resolve();
    });

    await serialPort.open();
    expect(serialPort.isOpen).toBe(true);

    await serialPort.enableAutoReconnect();
    const onData = jest.fn();
    await serialPort.watch({ onData });

    MockChannel.lastInstance!.onmessage?.({
      kind: 'disconnect',
      path: '/dev/tty.usbserial',
      reason: 'lost',
    });

    await new Promise<void>((r) => setTimeout(r, 0));

    expect(serialPort.isOpen).toBe(true);
    expect(watchCallCount).toBe(2);
    expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|unwatch', {
      channelId: 1,
    });

    MockChannel.lastInstance!.onmessage?.({
      kind: 'data',
      path: '/dev/tty.usbserial',
      data: [65],
      size: 1,
    });
    expect(onData).toHaveBeenCalledWith('A');
  });

  it('does not re-watch without enableAutoReconnect', async () => {
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'plugin:serialplugin|open') return Promise.resolve();
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

    await new Promise<void>((r) => setTimeout(r, 0));

    expect(serialPort.isOpen).toBe(false);
    expect(watchCallCount).toBe(1);
    const openCallsAfterDisconnect = mockInvoke.mock.calls.filter(
      (call) => call[0] === 'plugin:serialplugin|open',
    ).length;
    expect(openCallsAfterDisconnect).toBe(openCallsBeforeDisconnect);
  });
});
