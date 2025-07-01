import { SerialPort, DataBits, FlowControl, Parity, StopBits } from '../guest-js';
import { setupTestMocks, createTestSerialPort } from './test-utils';

describe('SerialPort Edge Cases', () => {
  let mockInvoke: ReturnType<typeof setupTestMocks>['mockInvoke'];
  let mockListen: ReturnType<typeof setupTestMocks>['mockListen'];
  let mockUnlisten: ReturnType<typeof setupTestMocks>['mockUnlisten'];
  let serialPort: SerialPort;

  beforeEach(() => {
    const mocks = setupTestMocks();
    mockInvoke = mocks.mockInvoke;
    mockListen = mocks.mockListen;
    mockUnlisten = mocks.mockUnlisten;
    serialPort = createTestSerialPort();
  });

  afterEach(async () => {
    await SerialPort.closeAll();
  });

  describe('Edge Cases', () => {
    it('should handle empty path in open', async () => {
      const port = new SerialPort({
        path: '',
        baudRate: 9600
      });
      await expect(port.open()).rejects.toEqual('path Can not be empty!');
    });

    it('should handle empty baudRate in open', async () => {
      const port = new SerialPort({
        path: '/dev/tty.usbserial',
        baudRate: 0
      });
      await expect(port.open()).rejects.toEqual('baudRate Can not be empty!');
    });

    it('should handle already open port in open', async () => {
      serialPort.isOpen = true;
      await expect(serialPort.open()).resolves.toBeUndefined();
      expect(mockInvoke).not.toHaveBeenCalled();
    });

    it('should handle already closed port in close', async () => {
      serialPort.isOpen = false;
      await expect(serialPort.close()).resolves.toBeUndefined();
      expect(mockInvoke).not.toHaveBeenCalled();
    });

    it('should handle invalid encoding in write', async () => {
      serialPort.isOpen = true;
      // Mock invoke to throw error on invalid encoding
      mockInvoke.mockRejectedValueOnce(new Error('Invalid encoding'));
      
      await expect(serialPort.write('test')).rejects.toThrow('Invalid encoding');
    });

    it('should handle invalid binary data in writeBinary', async () => {
      serialPort.isOpen = true;
      await expect(serialPort.writeBinary('invalid' as any)).rejects.toEqual(
        'value Argument type error! Expected type: string, Uint8Array, number[]'
      );
    });

    it('should handle error in cancelRead', async () => {
      serialPort.isOpen = true;
      mockInvoke.mockRejectedValueOnce(new Error('Cancel read error'));
      // close() should handle cancelRead errors internally
      await expect(serialPort.close()).resolves.toBeUndefined();
    });

    it('should handle error in cancelListen', async () => {
      serialPort.isOpen = true;
      const consoleSpy = jest.spyOn(console, 'warn').mockImplementation();
      
      // Create mock that throws error on call
      const mockUnlistenError = jest.fn().mockImplementation(() => {
        throw new Error('Cancel listen error');
      });
      
      // Set up a listener first
      const callback = jest.fn();
      mockListen.mockResolvedValueOnce(mockUnlistenError);
      await serialPort.listen(callback);
      
      // cancelListen should handle errors internally and not throw
      await expect(serialPort.cancelListen()).resolves.toBeUndefined();
      expect(mockUnlistenError).toHaveBeenCalled();
      expect(consoleSpy).toHaveBeenCalledWith('Error unlistening data listener data_1:', expect.any(Error));
      consoleSpy.mockRestore();
    });

    it('should handle error in startListening', async () => {
      serialPort.isOpen = true;
      mockInvoke.mockRejectedValueOnce(new Error('Start listening error'));
      await expect(serialPort.startListening()).rejects.toThrow('Start listening error');
    });

    it('should handle error in stopListening', async () => {
      serialPort.isOpen = true;
      mockInvoke.mockRejectedValueOnce(new Error('Stop listening error'));
      await expect(serialPort.stopListening()).rejects.toThrow('Stop listening error');
    });

    it('should handle error in listen callback', async () => {
      serialPort.isOpen = true;
      const consoleSpy = jest.spyOn(console, 'error').mockImplementation();
      const callback = jest.fn().mockImplementation(() => {
        throw new Error('Callback error');
      });
      mockListen.mockImplementationOnce((event, cb) => {
        cb({ payload: 'test' });
        return Promise.resolve(mockUnlisten);
      });
      await serialPort.listen(callback);
      expect(callback).toHaveBeenCalled();
      expect(consoleSpy).toHaveBeenCalledWith(expect.any(Error));
      consoleSpy.mockRestore();
    });

    it('should handle error in disconnected event', async () => {
      serialPort.isOpen = true;
      const consoleSpy = jest.spyOn(console, 'error').mockImplementation();
      
      // Create callback that throws error
      const errorCallback = jest.fn().mockImplementation(() => {
        throw new Error('Connection lost');
      });
      
      // Mock listen for disconnected event
      const portPath = '/dev/tty.usbserial';
      const subPath = portPath.replaceAll(".", "-").replaceAll("/", "-");
      const disconnectedEvent = `plugin-serialplugin-disconnected-${subPath}`;
      let eventCallback: ((event: any) => void) | undefined;
      
      // Add debug output to check calls
      mockListen.mockImplementationOnce((event, cb) => {
        if (event === disconnectedEvent) {
          eventCallback = cb;
        }
        return Promise.resolve(mockUnlisten);
      });

      // Mock invoke for successful port opening
      mockInvoke.mockResolvedValueOnce(undefined);
      
      // Open port and set disconnect handler
      await serialPort.open();
      
      // Check that disconnected method was called
      const disconnectedSpy = jest.spyOn(serialPort, 'disconnected');
      await serialPort.disconnected(errorCallback);
      expect(disconnectedSpy).toHaveBeenCalledWith(errorCallback);
      
      // Check that listen was called with correct event
      expect(mockListen).toHaveBeenCalledWith(disconnectedEvent, expect.any(Function));
      
      // Check that event handler was set
      expect(eventCallback).toBeDefined();
      
      // Simulate disconnect event
      if (eventCallback) {
        eventCallback({});
      }
      
      // Give time for event processing and check results
      await new Promise(process.nextTick);
      
      // Check that callback was called and error was logged
      expect(errorCallback).toHaveBeenCalled();
      expect(consoleSpy).toHaveBeenCalledWith(expect.any(Error));
      
      // Clear all mocks
      consoleSpy.mockRestore();
      disconnectedSpy.mockRestore();
    });

    it('should handle error in available_ports', async () => {
      mockInvoke.mockRejectedValueOnce(new Error('Available ports error'));
      await expect(SerialPort.available_ports()).rejects.toThrow('Available ports error');
    });

    it('should handle error in available_ports_direct', async () => {
      mockInvoke.mockRejectedValueOnce(new Error('Available ports direct error'));
      await expect(SerialPort.available_ports_direct()).rejects.toThrow('Available ports direct error');
    });

    it('should handle error in managed_ports', async () => {
      mockInvoke.mockRejectedValueOnce(new Error('Managed ports error'));
      await expect(SerialPort.managed_ports()).rejects.toThrow('Managed ports error');
    });

    it('should handle error in forceClose', async () => {
      mockInvoke.mockRejectedValueOnce(new Error('Force close error'));
      await expect(SerialPort.forceClose('/dev/tty.usbserial')).rejects.toThrow('Force close error');
    });

    it('should handle error in closeAll', async () => {
      mockInvoke.mockRejectedValueOnce(new Error('Close all error'));
      await expect(SerialPort.closeAll()).rejects.toThrow('Close all error');
    });

    it('should handle error in setBaudRate', async () => {
      serialPort.isOpen = true;
      mockInvoke.mockRejectedValueOnce(new Error('Set baud rate error'));
      await expect(serialPort.setBaudRate(9600)).rejects.toThrow('Set baud rate error');
    });

    it('should handle error in setDataBits', async () => {
      serialPort.isOpen = true;
      mockInvoke.mockRejectedValueOnce(new Error('Set data bits error'));
      await expect(serialPort.setDataBits(DataBits.Seven)).rejects.toThrow('Set data bits error');
    });

    it('should handle error in setFlowControl', async () => {
      serialPort.isOpen = true;
      mockInvoke.mockRejectedValueOnce(new Error('Set flow control error'));
      await expect(serialPort.setFlowControl(FlowControl.Hardware)).rejects.toThrow('Set flow control error');
    });

    it('should handle error in setParity', async () => {
      serialPort.isOpen = true;
      mockInvoke.mockRejectedValueOnce(new Error('Set parity error'));
      await expect(serialPort.setParity(Parity.Even)).rejects.toThrow('Set parity error');
    });

    it('should handle error in setStopBits', async () => {
      serialPort.isOpen = true;
      mockInvoke.mockRejectedValueOnce(new Error('Set stop bits error'));
      await expect(serialPort.setStopBits(StopBits.Two)).rejects.toThrow('Set stop bits error');
    });

    it('should handle error in setTimeout', async () => {
      serialPort.isOpen = true;
      mockInvoke.mockRejectedValueOnce(new Error('Set timeout error'));
      await expect(serialPort.setTimeout(1000)).rejects.toThrow('Set timeout error');
    });

    it('should handle error in setRequestToSend', async () => {
      serialPort.isOpen = true;
      mockInvoke.mockRejectedValueOnce(new Error('Set RTS error'));
      await expect(serialPort.setRequestToSend(true)).rejects.toThrow('Set RTS error');
    });

    it('should handle error in setDataTerminalReady', async () => {
      serialPort.isOpen = true;
      mockInvoke.mockRejectedValueOnce(new Error('Set DTR error'));
      await expect(serialPort.setDataTerminalReady(true)).rejects.toThrow('Set DTR error');
    });

    it('should handle error in readClearToSend', async () => {
      serialPort.isOpen = true;
      mockInvoke.mockRejectedValueOnce(new Error('Read CTS error'));
      await expect(serialPort.readClearToSend()).rejects.toThrow('Read CTS error');
    });

    it('should handle error in readDataSetReady', async () => {
      serialPort.isOpen = true;
      mockInvoke.mockRejectedValueOnce(new Error('Read DSR error'));
      await expect(serialPort.readDataSetReady()).rejects.toThrow('Read DSR error');
    });

    it('should handle error in readRingIndicator', async () => {
      serialPort.isOpen = true;
      mockInvoke.mockRejectedValueOnce(new Error('Read RI error'));
      await expect(serialPort.readRingIndicator()).rejects.toThrow('Read RI error');
    });

    it('should handle error in readCarrierDetect', async () => {
      serialPort.isOpen = true;
      mockInvoke.mockRejectedValueOnce(new Error('Read CD error'));
      await expect(serialPort.readCarrierDetect()).rejects.toThrow('Read CD error');
    });

    it('should handle error in bytesToRead', async () => {
      serialPort.isOpen = true;
      mockInvoke.mockRejectedValueOnce(new Error('Bytes to read error'));
      await expect(serialPort.bytesToRead()).rejects.toThrow('Bytes to read error');
    });

    it('should handle error in bytesToWrite', async () => {
      serialPort.isOpen = true;
      mockInvoke.mockRejectedValueOnce(new Error('Bytes to write error'));
      await expect(serialPort.bytesToWrite()).rejects.toThrow('Bytes to write error');
    });

    it('should handle error in clearBuffer', async () => {
      serialPort.isOpen = true;
      mockInvoke.mockRejectedValueOnce(new Error('Clear buffer error'));
      await expect(serialPort.clearBuffer('All' as any)).rejects.toThrow('Clear buffer error');
    });

    it('should handle error in setBreak', async () => {
      serialPort.isOpen = true;
      mockInvoke.mockRejectedValueOnce(new Error('Set break error'));
      await expect(serialPort.setBreak()).rejects.toThrow('Set break error');
    });

    it('should handle error in clearBreak', async () => {
      serialPort.isOpen = true;
      mockInvoke.mockRejectedValueOnce(new Error('Clear break error'));
      await expect(serialPort.clearBreak()).rejects.toThrow('Clear break error');
    });
  });
}); 
 