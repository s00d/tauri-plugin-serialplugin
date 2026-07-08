import { mockExchangeResponse } from './exchange-mock';
import { createTestSerialPort, setupTestMocks } from './test-utils';

describe('SerialPort exchange', () => {
  beforeEach(() => {
    setupTestMocks();
  });

  it('returns Uint8Array raw from exchange()', async () => {
    const { mockInvoke } = setupTestMocks();
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'plugin:serialplugin|open') return Promise.resolve('/dev/tty.usbserial');
      if (cmd === 'plugin:serialplugin|exchange') {
        return Promise.resolve(mockExchangeResponse('OK\r\n'));
      }
      return Promise.resolve();
    });

    const port = createTestSerialPort();
    await port.open();
    const result = await port.exchange('AT\r');
    expect(result.raw).toBeInstanceOf(Uint8Array);
    expect(result.raw.length).toBeGreaterThan(0);
  });

  it('returns Uint8Array raw from exchangeBinary()', async () => {
    const { mockInvoke } = setupTestMocks();
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'plugin:serialplugin|open') return Promise.resolve('/dev/tty.usbserial');
      if (cmd === 'plugin:serialplugin|exchange_binary') {
        return Promise.resolve(mockExchangeResponse('>\r\n'));
      }
      return Promise.resolve();
    });

    const port = createTestSerialPort();
    await port.open();
    const result = await port.exchangeBinary(new Uint8Array([0x1a]));
    expect(result.raw).toBeInstanceOf(Uint8Array);
  });

  it('rejects exchange when port is closed', async () => {
    const port = createTestSerialPort();
    await expect(port.exchange('AT')).rejects.toThrow('Port is not open');
  });
});
