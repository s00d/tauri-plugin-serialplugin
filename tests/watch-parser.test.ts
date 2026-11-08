// watch({ parser }): native framing passthrough, per-frame onData, reconnect keeps it.
import { MockChannel } from './setup';
import { setupTestMocks, createTestSerialPort, wire } from './test-utils';

const PATH = '/dev/tty.usbserial';

function mockPort() {
  const { mockInvoke } = setupTestMocks();
  let watchId = 0;
  mockInvoke.mockImplementation((cmd: string) => {
    if (cmd === 'plugin:serialplugin|open') return Promise.resolve(PATH);
    if (cmd === 'plugin:serialplugin|watch') return Promise.resolve(++watchId);
    return Promise.resolve();
  });
  return mockInvoke;
}

const watchCalls = (mock: ReturnType<typeof mockPort>) =>
  mock.mock.calls.filter((c) => c[0] === 'plugin:serialplugin|watch');

const emitData = (data: number[]) =>
  MockChannel.lastInstance!.onmessage?.({ kind: 'data', path: PATH, data, size: data.length });

describe('watch parser', () => {
  afterEach(() => vi.useRealTimers());

  it('sends the parser with Uint8Array fields as byte arrays', async () => {
    const mockInvoke = mockPort();
    const port = createTestSerialPort();
    await port.open();
    await port.watch(
      { onData: vi.fn() },
      {
        parser: {
          type: 'startEnd',
          startDelimiter: new Uint8Array([0x02]),
          endDelimiter: [0x03],
          includeEndDelimiter: true,
        },
      },
    );
    expect(watchCalls(mockInvoke)[0][1].options.parser).toEqual({
      type: 'startEnd',
      startDelimiter: [0x02],
      endDelimiter: [0x03],
      includeEndDelimiter: true,
    });
  });

  it('omits parser when not given (unchanged payload)', async () => {
    const mockInvoke = mockPort();
    const port = createTestSerialPort();
    await port.open();
    await port.watch({ onData: vi.fn() });
    expect(watchCalls(mockInvoke)[0][1].options).not.toHaveProperty('parser');
  });

  it('calls onData once per frame, each decoded on its own', async () => {
    mockPort();
    const port = createTestSerialPort();
    await port.open();
    const onData = vi.fn();
    await port.watch({ onData }, { parser: { type: 'readline' } });
    const ru = wire('привет');
    emitData(wire('first'));
    // A frame ending mid-character is not held for the next frame.
    emitData(ru.slice(0, 3));
    emitData(wire('second'));
    expect(onData.mock.calls.map((c) => c[0])).toEqual(['first', 'п�', 'second']);
  });

  it('passes frames as Uint8Array with decode: false', async () => {
    mockPort();
    const port = createTestSerialPort();
    await port.open();
    const onData = vi.fn();
    await port.watch({ onData }, { decode: false, parser: { type: 'byteLength', length: 2 } });
    emitData([1, 2]);
    emitData([3, 4]);
    expect(onData).toHaveBeenCalledTimes(2);
    expect(onData.mock.calls[1][0]).toEqual(new Uint8Array([3, 4]));
  });

  it('rejects watch() when the native side refuses the parser', async () => {
    const { mockInvoke } = setupTestMocks();
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'plugin:serialplugin|open') return Promise.resolve(PATH);
      if (cmd === 'plugin:serialplugin|watch')
        return Promise.reject(new Error('invalid parser options: "length" must be greater than 0'));
      return Promise.resolve();
    });
    const port = createTestSerialPort();
    await port.open();
    await expect(
      port.watch({ onData: vi.fn() }, { parser: { type: 'byteLength', length: 0 } }),
    ).rejects.toThrow('invalid parser options');
    expect(port.activeWatch).toBeNull();
  });

  it('auto-reconnect re-establishes the watch with the same parser', async () => {
    vi.useFakeTimers();
    const mockInvoke = mockPort();
    const port = createTestSerialPort();
    await port.open();
    port.enableAutoReconnect({ interval: 100 });
    const parser = { type: 'interByteTimeout', intervalMs: 30 } as const;
    await port.watch({ onData: vi.fn() }, { parser });
    MockChannel.lastInstance!.onmessage?.({ kind: 'disconnect', path: PATH, reason: 'lost' });
    await vi.runOnlyPendingTimersAsync();
    const calls = watchCalls(mockInvoke);
    expect(calls).toHaveLength(2);
    expect(calls[1][1].options.parser).toEqual(parser);
  });
});
