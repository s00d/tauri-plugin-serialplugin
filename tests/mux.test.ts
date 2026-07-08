import { SerialPort } from '../guest-js';
import { createTestSerialPort, setupTestMocks } from './test-utils';

describe('SerialPort CMUX', () => {
  beforeEach(() => {
    setupTestMocks();
  });

  it('enableMux invokes native command', async () => {
    const { mockInvoke } = setupTestMocks();
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'plugin:serialplugin|open') return Promise.resolve('/dev/tty.usbserial');
      if (cmd === 'plugin:serialplugin|enable_mux') return Promise.resolve();
      return Promise.resolve();
    });

    const port = createTestSerialPort();
    await port.open();
    await port.enableMux({ command: 'AT+CMUX=0', timeoutMs: 3000 });
    expect(mockInvoke).toHaveBeenCalledWith(
      'plugin:serialplugin|enable_mux',
      expect.objectContaining({ path: '/dev/tty.usbserial' }),
    );
  });

  it('openMuxChannel returns a new SerialPort with virtual path', async () => {
    const { mockInvoke } = setupTestMocks();
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'plugin:serialplugin|open') return Promise.resolve('/dev/tty.usbserial');
      if (cmd === 'plugin:serialplugin|open_mux_channel') {
        return Promise.resolve('/dev/tty.usbserial#dlci=2');
      }
      return Promise.resolve();
    });

    const port = createTestSerialPort();
    await port.open();
    const virtual = await port.openMuxChannel(2);
    expect(virtual).toBeInstanceOf(SerialPort);
    expect(virtual.options.path).toBe('/dev/tty.usbserial#dlci=2');
  });

  it('disableMux is a no-op when port is closed', async () => {
    const port = createTestSerialPort();
    await expect(port.disableMux()).resolves.toBeUndefined();
  });
});
