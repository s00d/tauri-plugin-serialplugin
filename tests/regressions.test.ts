// One check per bug fixed in the 4.0 audit.
import { MockChannel } from './setup';
import { setupTestMocks, createTestSerialPort } from './test-utils';

const PATH = '/dev/tty.usbserial';
const calls = (mock: ReturnType<typeof setupTestMocks>['mockInvoke'], cmd: string) =>
  mock.mock.calls.filter((c) => c[0] === `plugin:serialplugin|${cmd}`);

function mockPort(extra: (cmd: string, args?: any) => unknown = () => undefined) {
  const { mockInvoke } = setupTestMocks();
  let watchId = 0;
  mockInvoke.mockImplementation((cmd: string, args?: any) => {
    const custom = extra(cmd, args);
    if (custom !== undefined) return custom as Promise<unknown>;
    if (cmd === 'plugin:serialplugin|open') return Promise.resolve(PATH);
    if (cmd === 'plugin:serialplugin|watch') return Promise.resolve(++watchId);
    return Promise.resolve();
  });
  return mockInvoke;
}

const disconnect = () =>
  MockChannel.lastInstance!.onmessage?.({ kind: 'disconnect', path: PATH, reason: 'unplugged' });

describe('audit regressions', () => {
  afterEach(() => vi.useRealTimers());

  it('sendAtPhases sends binary phases as a byte array, not an object', async () => {
    const mockInvoke = mockPort((cmd) =>
      cmd === 'plugin:serialplugin|at_phases' ? Promise.resolve([]) : undefined,
    );
    const port = createTestSerialPort();
    await port.open();
    await port.sendAtPhases([{ write: 'AT+CMGS=3' }, { write: new Uint8Array([1, 2, 0x1a]) }]);
    const phases = calls(mockInvoke, 'at_phases')[0][1].phases;
    expect(phases[1].write).toEqual([1, 2, 0x1a]);
  });

  it('handle.unwatch() frees the port for a new watch', async () => {
    mockPort();
    const port = createTestSerialPort();
    await port.open();
    const handle = await port.watch({ onData: vi.fn() });
    await handle.unwatch();
    expect(port.activeWatch).toBeNull();
    await expect(port.watch({ onData: vi.fn() })).resolves.toBeDefined();
  });

  it('close() after a disconnect stops auto-reconnect and closes natively', async () => {
    vi.useFakeTimers();
    const mockInvoke = mockPort((cmd) =>
      cmd === 'plugin:serialplugin|open' && calls(mockInvoke, 'open').length > 1
        ? Promise.reject(new Error('device gone'))
        : undefined,
    );
    const port = createTestSerialPort();
    await port.open();
    port.enableAutoReconnect({ interval: 100 });
    await port.watch({ onData: vi.fn() });
    disconnect();
    await vi.advanceTimersByTimeAsync(0);
    await port.close();
    const opensAfterClose = calls(mockInvoke, 'open').length;
    await vi.advanceTimersByTimeAsync(1000);
    expect(calls(mockInvoke, 'open').length).toBe(opensAfterClose);
    expect(calls(mockInvoke, 'close').length).toBe(1);
  });

  it('set* methods update options used by the next reopen', async () => {
    const mockInvoke = mockPort();
    const port = createTestSerialPort({ baudRate: 9600 });
    await port.open();
    await port.setBaudRate(115200);
    await port.change({});
    expect(calls(mockInvoke, 'open').at(-1)![1].baudRate).toBe(115200);
  });

  it('open() that fails in configureAtSession leaves the port closed', async () => {
    const mockInvoke = mockPort((cmd) =>
      cmd === 'plugin:serialplugin|configure_at_session'
        ? Promise.reject(new Error('bad session'))
        : undefined,
    );
    const port = createTestSerialPort({ atSession: { appendCr: true } });
    await expect(port.open()).rejects.toThrow('bad session');
    expect(port.isOpen).toBe(false);
    expect(calls(mockInvoke, 'close').length).toBe(1);
  });

  it('auto-reconnect retries when the re-watch fails after a successful open', async () => {
    vi.useFakeTimers();
    let failWatch = false;
    const mockInvoke = mockPort((cmd) =>
      cmd === 'plugin:serialplugin|watch' && failWatch
        ? ((failWatch = false), Promise.reject(new Error('watch failed')))
        : undefined,
    );
    const port = createTestSerialPort();
    const onReconnect = vi.fn();
    await port.open();
    port.enableAutoReconnect({ interval: 100, onReconnect });
    await port.watch({ onData: vi.fn() });
    failWatch = true;
    disconnect();
    await vi.advanceTimersByTimeAsync(500);
    expect(onReconnect).toHaveBeenLastCalledWith(true, 2);
    expect(calls(mockInvoke, 'watch').length).toBe(3);
    expect(port.isOpen).toBe(true);
  });

  it('concurrent watch() calls start only one native watch', async () => {
    const mockInvoke = mockPort();
    const port = createTestSerialPort();
    await port.open();
    const results = await Promise.allSettled([
      port.watch({ onData: vi.fn() }),
      port.watch({ onData: vi.fn() }),
    ]);
    expect(results.filter((r) => r.status === 'fulfilled')).toHaveLength(1);
    expect(calls(mockInvoke, 'watch')).toHaveLength(1);
  });

  it('watch() does not use the 1 s read timeout as its batch window', async () => {
    const mockInvoke = mockPort();
    const port = createTestSerialPort({ timeout: 1000 });
    await port.open();
    await port.watch({ onData: vi.fn() });
    expect(calls(mockInvoke, 'watch')[0][1].options.timeout).toBeUndefined();
  });
});
