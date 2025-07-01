import { SerialPort, DataBits, FlowControl, Parity, StopBits } from '../guest-js';
import { setupTestMocks, createTestSerialPort } from './test-utils';

describe('SerialPort Operations', () => {
  let mockInvoke: ReturnType<typeof setupTestMocks>['mockInvoke'];
  let serialPort: SerialPort;

  beforeEach(() => {
    const mocks = setupTestMocks();
    mockInvoke = mocks.mockInvoke;
    serialPort = createTestSerialPort();
  });

  afterEach(async () => {
    await SerialPort.closeAll();
  });

  describe('open', () => {
    it('should open port successfully', async () => {
      mockInvoke.mockResolvedValueOnce(undefined);

      await serialPort.open();
      expect(serialPort.isOpen).toBe(true);
      expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|open', {
        path: '/dev/tty.usbserial',
        baudRate: 9600,
        dataBits: DataBits.Eight,
        flowControl: FlowControl.None,
        parity: Parity.None,
        stopBits: StopBits.One,
        timeout: 1000
      });
    });

    it('should handle open errors', async () => {
      const error = new Error('Failed to open port');
      mockInvoke.mockRejectedValueOnce(error);

      await expect(serialPort.open()).rejects.toThrow('Failed to open port');
      expect(serialPort.isOpen).toBe(false);
    });
  });

  describe('close', () => {
    it('should close port successfully', async () => {
      serialPort.isOpen = true;
      mockInvoke.mockResolvedValueOnce(undefined);

      await serialPort.close();
      expect(serialPort.isOpen).toBe(false);
      expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|close', {
        path: '/dev/tty.usbserial'
      });
    });

    it('should handle close errors', async () => {
      serialPort.isOpen = true;
      const error = new Error('Failed to close port');
      mockInvoke.mockRejectedValueOnce(error);

      // close() should not throw errors, it should handle them internally
      await expect(serialPort.close()).resolves.toBeUndefined();
      expect(serialPort.isOpen).toBe(false);
    });
  });

  describe('write', () => {
    beforeEach(() => {
      serialPort.isOpen = false;
    });

    it('should write data successfully', async () => {
      serialPort.isOpen = true;
      mockInvoke.mockResolvedValueOnce(5);

      const bytesWritten = await serialPort.write('Hello');
      expect(bytesWritten).toBe(5);
      expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|write', {
        path: '/dev/tty.usbserial',
        value: 'Hello'
      });
    });

    it('should throw error if port is not open', async () => {
      await expect(serialPort.write('Hello')).rejects.toEqual(`serial port ${serialPort.options.path} not opened!`);
    });
  });

  describe('read', () => {
    beforeEach(() => {
      serialPort.isOpen = false;
    });

    it('should read data successfully', async () => {
      serialPort.isOpen = true;
      mockInvoke.mockResolvedValueOnce('Hello');

      const data = await serialPort.read();
      expect(data).toBe('Hello');
      expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|read', {
        path: '/dev/tty.usbserial',
        timeout: 1000,
        size: 1024
      });
    });

    it('should read with custom options', async () => {
      serialPort.isOpen = true;
      mockInvoke.mockResolvedValueOnce('Hello');

      const data = await serialPort.read({ timeout: 500, size: 2048 });
      expect(data).toBe('Hello');
      expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|read', {
        path: '/dev/tty.usbserial',
        timeout: 500,
        size: 2048
      });
    });

    it('should throw error if port is not open', async () => {
      await expect(serialPort.read()).rejects.toEqual('Port is not open');
    });
  });

  describe('readBinary', () => {
    beforeEach(() => {
      serialPort.isOpen = false;
    });

    it('should read binary data successfully', async () => {
      serialPort.isOpen = true;
      const mockData = [1, 2, 3, 4, 5];
      mockInvoke.mockResolvedValueOnce(mockData);

      const data = await serialPort.readBinary();
      expect(data).toEqual(new Uint8Array(mockData));
      expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|read_binary', {
        path: '/dev/tty.usbserial',
        timeout: 1000,
        size: 1024
      });
    });

    it('should read binary data with custom options', async () => {
      serialPort.isOpen = true;
      const mockData = [1, 2, 3, 4, 5];
      mockInvoke.mockResolvedValueOnce(mockData);

      const data = await serialPort.readBinary({ timeout: 500, size: 2048 });
      expect(data).toEqual(new Uint8Array(mockData));
      expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|read_binary', {
        path: '/dev/tty.usbserial',
        timeout: 500,
        size: 2048
      });
    });
  });

  describe('writeBinary', () => {
    beforeEach(() => {
      serialPort.isOpen = false;
    });

    it('should write binary data successfully', async () => {
      serialPort.isOpen = true;
      const data = new Uint8Array([1, 2, 3, 4, 5]);
      mockInvoke.mockResolvedValueOnce(data.length);

      const bytesWritten = await serialPort.writeBinary(data);
      expect(bytesWritten).toBe(data.length);
      expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|write_binary', {
        path: '/dev/tty.usbserial',
        value: [1, 2, 3, 4, 5]
      });
    });

    it('should handle array input', async () => {
      serialPort.isOpen = true;
      const data = [1, 2, 3, 4, 5];
      mockInvoke.mockResolvedValueOnce(data.length);
      const bytesWritten = await serialPort.writeBinary(data);
      expect(bytesWritten).toBe(data.length);
      expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|write_binary', {
        value: data,
        path: '/dev/tty.usbserial'
      });
    });

    it('should handle invalid input type', async () => {
      serialPort.isOpen = true;
      await expect(serialPort.writeBinary('invalid' as any)).rejects.toEqual(
        'value Argument type error! Expected type: string, Uint8Array, number[]'
      );
    });

    it('should throw error if port is not open', async () => {
      const data = new Uint8Array([1, 2, 3]);
      await expect(serialPort.writeBinary(data)).rejects.toEqual(`serial port ${serialPort.options.path} not opened!`);
    });
  });

  describe('startListening and stopListening', () => {
    beforeEach(() => {
      serialPort.isOpen = true;
    });

    it('should start listening successfully', async () => {
      mockInvoke.mockResolvedValueOnce('started');

      await serialPort.startListening();
      expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|start_listening', {
        path: '/dev/tty.usbserial',
        size: 1024,
        timeout: 1000
      });
    });

    it('should stop listening successfully', async () => {
      mockInvoke.mockResolvedValueOnce('stopped');

      await serialPort.stopListening();
      expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|stop_listening', {
        path: '/dev/tty.usbserial'
      });
    });
  });
}); 
