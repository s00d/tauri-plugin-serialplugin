import { SerialPort } from '../guest-js';
import { MockChannel } from './setup';
import { setupTestMocks, createTestSerialPort } from './test-utils';

describe('SerialPort Encoding Operations', () => {
  let mockInvoke: ReturnType<typeof setupTestMocks>['mockInvoke'];
  let serialPort: SerialPort;

  beforeEach(() => {
    const mocks = setupTestMocks();
    mockInvoke = mocks.mockInvoke;
    serialPort = createTestSerialPort();
    serialPort.isOpen = true;
  });

  afterEach(async () => {
    await SerialPort.closeAll();
  });

  describe('Encoding Operations', () => {
    it('should write and read with different encodings', async () => {
      serialPort.encoding = 'utf-8';
      const utf8Data = 'Hello, world! 🚀';

      mockInvoke.mockImplementationOnce(() => Promise.resolve(utf8Data.length));
      const writeResult = await serialPort.write(utf8Data);
      expect(writeResult).toBe(utf8Data.length);

      mockInvoke.mockImplementationOnce(() => Promise.resolve(utf8Data));
      const readResult = await serialPort.read();
      expect(readResult).toBe(utf8Data);

      serialPort.encoding = 'ascii';
      const asciiData = 'Hello, World!';

      mockInvoke.mockImplementationOnce(() => Promise.resolve(asciiData.length));
      const asciiWriteResult = await serialPort.write(asciiData);
      expect(asciiWriteResult).toBe(asciiData.length);

      mockInvoke.mockImplementationOnce(() => Promise.resolve(asciiData));
      const asciiReadResult = await serialPort.read();
      expect(asciiReadResult).toBe(asciiData);

      const binaryData = new Uint8Array([0x01, 0x02, 0x03, 0x04, 0x05]);

      mockInvoke.mockImplementationOnce(() => Promise.resolve(binaryData.length));
      const binaryWriteResult = await serialPort.writeBinary(binaryData);
      expect(binaryWriteResult).toBe(binaryData.length);

      mockInvoke.mockImplementationOnce(() => Promise.resolve(Array.from(binaryData)));
      const binaryReadResult = await serialPort.readBinary();
      expect(binaryReadResult).toEqual(binaryData);

      expect(mockInvoke).toHaveBeenCalledTimes(6);
    });

    it('should handle encoding errors', async () => {
      serialPort.encoding = 'ascii';
      const invalidData = 'café';

      mockInvoke.mockImplementationOnce(() => Promise.reject(new Error('Invalid encoding')));
      await expect(serialPort.write(invalidData)).rejects.toThrow('Invalid encoding');
    });

    it('should decode watch data with configured encoding', async () => {
      mockInvoke.mockImplementation((cmd: string) => {
        if (cmd === 'plugin:serialplugin|watch') return Promise.resolve(1);
        return Promise.resolve();
      });

      const callback = jest.fn();
      await serialPort.watch({ onData: callback });

      MockChannel.lastInstance!.onmessage?.({
        kind: 'data',
        path: '/dev/tty.usbserial',
        data: [72, 101, 108, 108, 111],
        size: 5,
      });

      expect(callback).toHaveBeenCalledWith('Hello');
    });

    it('should pass binary data when decode is false', async () => {
      mockInvoke.mockImplementation((cmd: string) => {
        if (cmd === 'plugin:serialplugin|watch') return Promise.resolve(1);
        return Promise.resolve();
      });

      const callback = jest.fn();
      await serialPort.watch({ onData: callback }, { decode: false });

      MockChannel.lastInstance!.onmessage?.({
        kind: 'data',
        path: '/dev/tty.usbserial',
        data: [1, 2, 3, 4, 5],
        size: 5,
      });

      expect(callback).toHaveBeenCalledWith(new Uint8Array([1, 2, 3, 4, 5]));
    });
  });
});
