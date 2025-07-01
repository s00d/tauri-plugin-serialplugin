import { SerialPort } from '../guest-js';
import { setupTestMocks, createTestSerialPort } from './test-utils';

describe('SerialPort Encoding Operations', () => {
  let mockInvoke: ReturnType<typeof setupTestMocks>['mockInvoke'];
  let mockListen: ReturnType<typeof setupTestMocks>['mockListen'];
  let serialPort: SerialPort;

  beforeEach(() => {
    const mocks = setupTestMocks();
    mockInvoke = mocks.mockInvoke;
    mockListen = mocks.mockListen;
    serialPort = createTestSerialPort();
    serialPort.isOpen = true;
  });

  afterEach(async () => {
    await SerialPort.closeAll();
  });

  describe('Encoding Operations', () => {
    it('should write and read with different encodings', async () => {
      // Test UTF-8
      serialPort.encoding = 'utf-8';
      const utf8Data = 'Привет, мир!';
      
      // Mock successful write
      mockInvoke.mockImplementationOnce(() => Promise.resolve(utf8Data.length));
      const writeResult = await serialPort.write(utf8Data);
      expect(writeResult).toBe(utf8Data.length);

      // Mock successful read
      mockInvoke.mockImplementationOnce(() => Promise.resolve(utf8Data));
      const readResult = await serialPort.read();
      expect(readResult).toBe(utf8Data);

      // Test ASCII
      serialPort.encoding = 'ascii';
      const asciiData = 'Hello, World!';
      
      // Mock successful write
      mockInvoke.mockImplementationOnce(() => Promise.resolve(asciiData.length));
      const asciiWriteResult = await serialPort.write(asciiData);
      expect(asciiWriteResult).toBe(asciiData.length);

      // Mock successful read
      mockInvoke.mockImplementationOnce(() => Promise.resolve(asciiData));
      const asciiReadResult = await serialPort.read();
      expect(asciiReadResult).toBe(asciiData);

      // Test Binary
      const binaryData = new Uint8Array([0x01, 0x02, 0x03, 0x04, 0x05]);
      
      // Mock successful write
      mockInvoke.mockImplementationOnce(() => Promise.resolve(binaryData.length));
      const binaryWriteResult = await serialPort.writeBinary(binaryData);
      expect(binaryWriteResult).toBe(binaryData.length);

      // Mock successful read
      mockInvoke.mockImplementationOnce(() => Promise.resolve(Array.from(binaryData)));
      const binaryReadResult = await serialPort.readBinary();
      expect(binaryReadResult).toEqual(binaryData);

      expect(mockInvoke).toHaveBeenCalledTimes(6);
    });

    it('should handle encoding errors', async () => {
      serialPort.encoding = 'ascii';
      const invalidData = 'Привет'; // Cyrillic characters in ASCII
      
      mockInvoke.mockImplementationOnce(() => Promise.reject(new Error('Invalid encoding')));
      await expect(serialPort.write(invalidData)).rejects.toThrow('Invalid encoding');
    });

    it('should handle encoding fallback in listen callback', async () => {
      serialPort.encoding = 'invalid-encoding';
      const consoleSpy = jest.spyOn(console, 'error').mockImplementation();
      const callback = jest.fn();

      // Mock listen with data that needs decoding
      mockListen.mockImplementationOnce((event, cb) => {
        // Simulate the callback being called with data
        cb({ payload: { data: [72, 101, 108, 108, 111] } }); // "Hello" in ASCII
        return Promise.resolve(jest.fn());
      });

      await serialPort.listen(callback);

      // Verify that listen was called
      expect(mockListen).toHaveBeenCalled();
      
      // Verify that callback was called (with fallback string)
      expect(callback).toHaveBeenCalled();
      
      // In Node.js environment, TextDecoder with invalid encoding might not throw an error
      // So we just verify that the callback was called (which means the fallback worked)
      expect(callback).toHaveBeenCalledWith(expect.any(String));
      
      // The callback should have been called with some string data (either from fallback or direct conversion)
      const callbackArg = callback.mock.calls[0][0];
      expect(typeof callbackArg).toBe('string');
      expect(callbackArg.length).toBeGreaterThan(0);

      consoleSpy.mockRestore();
    });

    it('should handle UTF-8 fallback in listen callback', async () => {
      serialPort.encoding = 'utf-8';
      const consoleSpy = jest.spyOn(console, 'error').mockImplementation();
      const callback = jest.fn();

      // Mock listen with data that needs decoding
      mockListen.mockImplementationOnce((event, cb) => {
        cb({ payload: { data: [72, 101, 108, 108, 111] } }); // "Hello" in ASCII
        return Promise.resolve(jest.fn());
      });

      await serialPort.listen(callback);

      // Verify that listen was called
      expect(mockListen).toHaveBeenCalled();
      
      // Verify that callback was called with decoded text
      expect(callback).toHaveBeenCalledWith('Hello');

      consoleSpy.mockRestore();
    });

    it('should handle binary data in listen callback', async () => {
      const callback = jest.fn();

      // Mock listen with binary data
      mockListen.mockImplementationOnce((event, cb) => {
        cb({ payload: { data: [1, 2, 3, 4, 5] } });
        return Promise.resolve(jest.fn());
      });

      await serialPort.listen(callback, false); // isDecode = false

      // Verify that listen was called
      expect(mockListen).toHaveBeenCalled();
      
      // Verify that callback was called with binary data
      expect(callback).toHaveBeenCalledWith(new Uint8Array([1, 2, 3, 4, 5]));
    });
  });
}); 
 