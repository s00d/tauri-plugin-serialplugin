import { SerialPort } from '../guest-js';
import { setupTestMocks, createTestSerialPort } from './test-utils';

describe('SerialPort Control Signals', () => {
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

  describe('Control Signals', () => {
    beforeEach(() => {
      serialPort.isOpen = true;
    });

    it('should read CTS', async () => {
      mockInvoke.mockResolvedValueOnce(true);
      const result = await serialPort.readClearToSend();
      expect(result).toBe(true);
      expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|read_clear_to_send', {
        path: '/dev/tty.usbserial'
      });
    });

    it('should read DSR', async () => {
      mockInvoke.mockResolvedValueOnce(true);
      const result = await serialPort.readDataSetReady();
      expect(result).toBe(true);
      expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|read_data_set_ready', {
        path: '/dev/tty.usbserial'
      });
    });

    it('should read ring indicator', async () => {
      mockInvoke.mockResolvedValueOnce(true);
      const result = await serialPort.readRingIndicator();
      expect(result).toBe(true);
      expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|read_ring_indicator', {
        path: '/dev/tty.usbserial'
      });
    });

    it('should read carrier detect', async () => {
      mockInvoke.mockResolvedValueOnce(true);
      const result = await serialPort.readCarrierDetect();
      expect(result).toBe(true);
      expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|read_carrier_detect', {
        path: '/dev/tty.usbserial'
      });
    });

    describe('writeRequestToSend', () => {
      it('should set RTS to true', async () => {
        await serialPort.writeRequestToSend(true);
        expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|write_request_to_send', {
          path: '/dev/tty.usbserial',
          level: true
        });
      });

      it('should set RTS to false', async () => {
        await serialPort.writeRequestToSend(false);
        expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|write_request_to_send', {
          path: '/dev/tty.usbserial',
          level: false
        });
      });

      it('should handle RTS errors', async () => {
        mockInvoke.mockRejectedValueOnce(new Error('RTS error'));
        await expect(serialPort.writeRequestToSend(true)).rejects.toThrow('RTS error');
      });

      it('should throw error if port is not open', async () => {
        serialPort.isOpen = false;
        await expect(serialPort.writeRequestToSend(true)).resolves.toBeUndefined();
      });
    });

    describe('writeDataTerminalReady', () => {
      it('should set DTR to true', async () => {
        await serialPort.writeDataTerminalReady(true);
        expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|write_data_terminal_ready', {
          path: '/dev/tty.usbserial',
          level: true
        });
      });

      it('should set DTR to false', async () => {
        await serialPort.writeDataTerminalReady(false);
        expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|write_data_terminal_ready', {
          path: '/dev/tty.usbserial',
          level: false
        });
      });

      it('should handle DTR errors', async () => {
        mockInvoke.mockRejectedValueOnce(new Error('DTR error'));
        await expect(serialPort.writeDataTerminalReady(true)).rejects.toThrow('DTR error');
      });

      it('should throw error if port is not open', async () => {
        serialPort.isOpen = false;
        await expect(serialPort.writeDataTerminalReady(true)).resolves.toBeUndefined();
      });
    });
  });
}); 
