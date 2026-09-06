import { createTestSerialPort, setupTestMocks } from './test-utils';
import { mockExchangeResponse } from './exchange-mock';

describe('SerialPort exchange options payload', () => {
  beforeEach(() => {
    setupTestMocks();
  });

  function mockOpenAndExchange(mockInvoke: jest.Mock) {
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'plugin:serialplugin|open') {
        return Promise.resolve('/dev/tty.usbserial');
      }
      if (cmd === 'plugin:serialplugin|exchange') {
        return Promise.resolve(mockExchangeResponse('OK\r\n'));
      }
      if (cmd === 'plugin:serialplugin|exchange_binary') {
        return Promise.resolve(mockExchangeResponse('>\r\n'));
      }
      return Promise.resolve();
    });
  }

  it('forwards timeoutMs and completionMode', async () => {
    const { mockInvoke } = setupTestMocks();
    mockOpenAndExchange(mockInvoke);
    const port = createTestSerialPort();
    await port.open();
    await port.exchange('AT', {
      timeoutMs: 2500,
      completionMode: 'atFinalLine',
    });
    expect(mockInvoke).toHaveBeenCalledWith(
      'plugin:serialplugin|exchange',
      expect.objectContaining({
        options: expect.objectContaining({
          timeoutMs: 2500,
          completionMode: 'atFinalLine',
        }),
      }),
    );
  });

  it('forwards resultFormat and solicitedPrefixes', async () => {
    const { mockInvoke } = setupTestMocks();
    mockOpenAndExchange(mockInvoke);
    const port = createTestSerialPort();
    await port.open();
    await port.exchange('AT+CMGS=1', {
      resultFormat: 'numeric',
      solicitedPrefixes: ['+CMGS:'],
    });
    expect(mockInvoke).toHaveBeenCalledWith(
      'plugin:serialplugin|exchange',
      expect.objectContaining({
        options: expect.objectContaining({
          resultFormat: 'numeric',
          solicitedPrefixes: ['+CMGS:'],
        }),
      }),
    );
  });

  it('forwards rxPrepare and drain timings', async () => {
    const { mockInvoke } = setupTestMocks();
    mockOpenAndExchange(mockInvoke);
    const port = createTestSerialPort();
    await port.open();
    await port.exchange('AT', {
      rxPrepare: 'drain',
      drainIdleMs: 40,
      drainMaxMs: 200,
    });
    expect(mockInvoke).toHaveBeenCalledWith(
      'plugin:serialplugin|exchange',
      expect.objectContaining({
        options: expect.objectContaining({
          rxPrepare: 'drain',
          drainIdleMs: 40,
          drainMaxMs: 200,
        }),
      }),
    );
  });

  it('forwards idleMs and terminators', async () => {
    const { mockInvoke } = setupTestMocks();
    mockOpenAndExchange(mockInvoke);
    const port = createTestSerialPort();
    await port.open();
    await port.exchange('AT', {
      idleMs: 80,
      terminators: ['OK', 'ERROR'],
    });
    expect(mockInvoke).toHaveBeenCalledWith(
      'plugin:serialplugin|exchange',
      expect.objectContaining({
        options: expect.objectContaining({
          idleMs: 80,
          terminators: ['OK', 'ERROR'],
        }),
      }),
    );
  });

  it('sends empty options object when omitted', async () => {
    const { mockInvoke } = setupTestMocks();
    mockOpenAndExchange(mockInvoke);
    const port = createTestSerialPort();
    await port.open();
    await port.exchange('AT');
    expect(mockInvoke).toHaveBeenCalledWith(
      'plugin:serialplugin|exchange',
      expect.objectContaining({ options: {} }),
    );
  });

  it('forwards options on exchangeBinary', async () => {
    const { mockInvoke } = setupTestMocks();
    mockOpenAndExchange(mockInvoke);
    const port = createTestSerialPort();
    await port.open();
    await port.exchangeBinary(new Uint8Array([0x1a]), {
      timeoutMs: 1000,
      completionMode: 'atIntermediate',
    });
    expect(mockInvoke).toHaveBeenCalledWith(
      'plugin:serialplugin|exchange_binary',
      expect.objectContaining({
        options: expect.objectContaining({
          timeoutMs: 1000,
          completionMode: 'atIntermediate',
        }),
      }),
    );
  });

  it('forwards enableMux timeoutMs camelCase', async () => {
    const { mockInvoke } = setupTestMocks();
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'plugin:serialplugin|open') {
        return Promise.resolve('/dev/tty.usbserial');
      }
      return Promise.resolve();
    });
    const port = createTestSerialPort();
    await port.open();
    await port.enableMux({ command: 'AT+CMUX=0', timeoutMs: 3000 });
    expect(mockInvoke).toHaveBeenCalledWith(
      'plugin:serialplugin|enable_mux',
      expect.objectContaining({
        options: { command: 'AT+CMUX=0', timeoutMs: 3000 },
      }),
    );
  });
});
