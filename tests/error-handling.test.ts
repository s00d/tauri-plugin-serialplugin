import { SerialPort, DataBits, FlowControl, Parity, StopBits } from '../guest-js';
import { setupTestMocks, createTestSerialPort } from './test-utils';

describe('SerialPort Error Handling', () => {
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

  describe('Single Error Tests', () => {
    it('should handle port not found error on write', async () => {
      serialPort.isOpen = true;
      mockInvoke.mockImplementationOnce(() => Promise.reject(new Error('Port not found')));
      await expect(serialPort.write('test')).rejects.toThrow('Port not found');
    });

    it('should handle port not found error on read', async () => {
      serialPort.isOpen = true;
      mockInvoke.mockImplementationOnce(() => Promise.reject(new Error('Port not found')));
      await expect(serialPort.read()).rejects.toThrow('Port not found');
    });

    it('should handle port not found error on setBaudRate', async () => {
      serialPort.isOpen = true;
      mockInvoke.mockImplementationOnce(() => Promise.reject(new Error('Port not found')));
      await expect(serialPort.setBaudRate(9600)).rejects.toThrow('Port not found');
    });

    it('should handle permission denied error on open', async () => {
      serialPort.isOpen = false;
      mockInvoke.mockImplementationOnce(() => Promise.reject(new Error('Permission denied')));
      await expect(serialPort.open()).rejects.toThrow('Permission denied');
      expect(serialPort.isOpen).toBe(false);
    });

    it('should handle device busy error on open', async () => {
      serialPort.isOpen = false;
      mockInvoke.mockImplementationOnce(() => Promise.reject(new Error('Device busy')));
      await expect(serialPort.open()).rejects.toThrow('Device busy');
      expect(serialPort.isOpen).toBe(false);
    });

    it('should handle timeout error on read', async () => {
      serialPort.isOpen = true;
      mockInvoke.mockImplementationOnce(() => Promise.reject(new Error('Operation timed out')));
      await expect(serialPort.read()).rejects.toThrow('Operation timed out');
    });
  });

  describe('Sequential Error Tests', () => {
    it('should handle sequence of different errors', async () => {
      serialPort.isOpen = true;
      
      // Configure mock for error sequence
      mockInvoke
        .mockImplementationOnce(() => Promise.reject(new Error('Port not found')))
        .mockImplementationOnce(() => Promise.reject(new Error('Device busy')))
        .mockImplementationOnce(() => Promise.reject(new Error('Permission denied')));

      // Check each error in sequence
      await expect(serialPort.write('test')).rejects.toThrow('Port not found');
      await expect(serialPort.write('test')).rejects.toThrow('Device busy');
      await expect(serialPort.write('test')).rejects.toThrow('Permission denied');

      // Check that all mocks were called
      expect(mockInvoke).toHaveBeenCalledTimes(3);
    });

    it('should handle repeated errors', async () => {
      serialPort.isOpen = true;
      
      // Configure mock for repeated error
      mockInvoke
        .mockImplementationOnce(() => Promise.reject(new Error('Port not found')))
        .mockImplementationOnce(() => Promise.reject(new Error('Port not found')))
        .mockImplementationOnce(() => Promise.reject(new Error('Port not found')));

      // Check multiple calls with the same error
      for (let i = 0; i < 3; i++) {
        await expect(serialPort.write('test')).rejects.toThrow('Port not found');
      }

      expect(mockInvoke).toHaveBeenCalledTimes(3);
    });

    it('should handle alternating errors', async () => {
      serialPort.isOpen = true;
      
      // Configure mock for alternating errors
      mockInvoke
        .mockImplementationOnce(() => Promise.reject(new Error('Port not found')))
        .mockImplementationOnce(() => Promise.resolve(5))
        .mockImplementationOnce(() => Promise.reject(new Error('Port not found')))
        .mockImplementationOnce(() => Promise.resolve(5));

      // Check alternating errors and successful operations
      await expect(serialPort.write('test')).rejects.toThrow('Port not found');
      await expect(serialPort.write('test')).resolves.toBe(5);
      await expect(serialPort.write('test')).rejects.toThrow('Port not found');
      await expect(serialPort.write('test')).resolves.toBe(5);

      expect(mockInvoke).toHaveBeenCalledTimes(4);
    });
  });

  describe('Mixed Operation Tests', () => {
    let callCount: number;

    beforeEach(() => {
      callCount = 0;
      jest.clearAllMocks();
      serialPort.isOpen = false; // Start with closed port
    });

    afterEach(async () => {
      callCount = 0;
      jest.clearAllMocks();
      await SerialPort.closeAll();
    });

    it('should handle recovery after errors', async () => {
      mockInvoke.mockImplementation(() => {
        callCount++;
        switch (callCount) {
          case 1:
            return Promise.resolve(undefined); // open
          case 2:
            return Promise.resolve(5); // write
          case 3:
            return Promise.resolve(undefined); // open
          default:
            return Promise.resolve(undefined);
        }
      });

      // Check sequence with recovery
      await expect(serialPort.write('test')).rejects.toEqual(`serial port ${serialPort.options.path} not opened!`);
      expect(callCount).toBe(0); // invoke is not called because port is closed
      expect(serialPort.isOpen).toBe(false);
      
      await expect(serialPort.open()).resolves.toBeUndefined();
      expect(callCount).toBe(1);
      expect(serialPort.isOpen).toBe(true);
      
      await expect(serialPort.write('test')).resolves.toBe(5);
      expect(callCount).toBe(2);
      expect(serialPort.isOpen).toBe(true);
      
      serialPort.isOpen = false; // Simulate port closure
      await expect(serialPort.write('test')).rejects.toEqual(`serial port ${serialPort.options.path} not opened!`);
      expect(callCount).toBe(2); // invoke is not called because port is closed
      expect(serialPort.isOpen).toBe(false);
      
      await expect(serialPort.open()).resolves.toBeUndefined();
      expect(callCount).toBe(3);
      expect(serialPort.isOpen).toBe(true);

      expect(mockInvoke).toHaveBeenCalledTimes(3);
    });

    it('should handle multiple recovery cycles', async () => {
      mockInvoke.mockImplementation(() => {
        callCount++;
        if (callCount % 2 === 1) {
          return Promise.resolve(undefined); // open
        } else {
          return Promise.resolve(5); // write
        }
      });

      // Multiple open/write cycles
      for (let i = 0; i < 3; i++) {
        await expect(serialPort.open()).resolves.toBeUndefined();
        await expect(serialPort.write('test')).resolves.toBe(5);
        serialPort.isOpen = false; // Simulate port closure
      }

      expect(mockInvoke).toHaveBeenCalledTimes(6);
    });
  });

  describe('Error Handling in Specific Methods', () => {
    beforeEach(() => {
      serialPort.isOpen = true;
    });

    it('should handle error in setBaudRate', async () => {
      mockInvoke.mockRejectedValueOnce(new Error('Set baud rate error'));
      await expect(serialPort.setBaudRate(9600)).rejects.toThrow('Set baud rate error');
    });

    it('should handle error in setDataBits', async () => {
      mockInvoke.mockRejectedValueOnce(new Error('Set data bits error'));
      await expect(serialPort.setDataBits(DataBits.Seven)).rejects.toThrow('Set data bits error');
    });

    it('should handle error in setFlowControl', async () => {
      mockInvoke.mockRejectedValueOnce(new Error('Set flow control error'));
      await expect(serialPort.setFlowControl(FlowControl.Hardware)).rejects.toThrow('Set flow control error');
    });

    it('should handle error in setParity', async () => {
      mockInvoke.mockRejectedValueOnce(new Error('Set parity error'));
      await expect(serialPort.setParity(Parity.Even)).rejects.toThrow('Set parity error');
    });

    it('should handle error in setStopBits', async () => {
      mockInvoke.mockRejectedValueOnce(new Error('Set stop bits error'));
      await expect(serialPort.setStopBits(StopBits.Two)).rejects.toThrow('Set stop bits error');
    });

    it('should handle error in setTimeout', async () => {
      mockInvoke.mockRejectedValueOnce(new Error('Set timeout error'));
      await expect(serialPort.setTimeout(1000)).rejects.toThrow('Set timeout error');
    });

    it('should handle error in setRequestToSend', async () => {
      mockInvoke.mockRejectedValueOnce(new Error('Set RTS error'));
      await expect(serialPort.setRequestToSend(true)).rejects.toThrow('Set RTS error');
    });

    it('should handle error in setDataTerminalReady', async () => {
      mockInvoke.mockRejectedValueOnce(new Error('Set DTR error'));
      await expect(serialPort.setDataTerminalReady(true)).rejects.toThrow('Set DTR error');
    });

    it('should handle error in readClearToSend', async () => {
      mockInvoke.mockRejectedValueOnce(new Error('Read CTS error'));
      await expect(serialPort.readClearToSend()).rejects.toThrow('Read CTS error');
    });

    it('should handle error in readDataSetReady', async () => {
      mockInvoke.mockRejectedValueOnce(new Error('Read DSR error'));
      await expect(serialPort.readDataSetReady()).rejects.toThrow('Read DSR error');
    });

    it('should handle error in readRingIndicator', async () => {
      mockInvoke.mockRejectedValueOnce(new Error('Read RI error'));
      await expect(serialPort.readRingIndicator()).rejects.toThrow('Read RI error');
    });

    it('should handle error in readCarrierDetect', async () => {
      mockInvoke.mockRejectedValueOnce(new Error('Read CD error'));
      await expect(serialPort.readCarrierDetect()).rejects.toThrow('Read CD error');
    });

    it('should handle error in bytesToRead', async () => {
      mockInvoke.mockRejectedValueOnce(new Error('Bytes to read error'));
      await expect(serialPort.bytesToRead()).rejects.toThrow('Bytes to read error');
    });

    it('should handle error in bytesToWrite', async () => {
      mockInvoke.mockRejectedValueOnce(new Error('Bytes to write error'));
      await expect(serialPort.bytesToWrite()).rejects.toThrow('Bytes to write error');
    });

    it('should handle error in clearBuffer', async () => {
      mockInvoke.mockRejectedValueOnce(new Error('Clear buffer error'));
      await expect(serialPort.clearBuffer('All' as any)).rejects.toThrow('Clear buffer error');
    });

    it('should handle error in setBreak', async () => {
      mockInvoke.mockRejectedValueOnce(new Error('Set break error'));
      await expect(serialPort.setBreak()).rejects.toThrow('Set break error');
    });

    it('should handle error in clearBreak', async () => {
      mockInvoke.mockRejectedValueOnce(new Error('Clear break error'));
      await expect(serialPort.clearBreak()).rejects.toThrow('Clear break error');
    });

    it('should handle error in readBinary', async () => {
      mockInvoke.mockRejectedValueOnce(new Error('Read binary error'));
      await expect(serialPort.readBinary()).rejects.toThrow('Read binary error');
    });

    it('should handle error in writeBinary', async () => {
      const data = new Uint8Array([1, 2, 3]);
      mockInvoke.mockRejectedValueOnce(new Error('Write binary error'));
      await expect(serialPort.writeBinary(data)).rejects.toThrow('Write binary error');
    });
  });
}); 
