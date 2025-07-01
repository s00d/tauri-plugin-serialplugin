import { SerialPort, DataBits, FlowControl, Parity, StopBits, ClearBuffer } from '../guest-js';
import { setupTestMocks, createTestSerialPort } from './test-utils';

describe('SerialPort Settings', () => {
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

  describe('Port Settings', () => {
    beforeEach(() => {
      serialPort.isOpen = true;
    });

    it('should set baud rate', async () => {
      await serialPort.setBaudRate(115200);
      expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|set_baud_rate', {
        path: '/dev/tty.usbserial',
        baudRate: 115200
      });
    });

    it('should set data bits', async () => {
      await serialPort.setDataBits(DataBits.Seven);
      expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|set_data_bits', {
        path: '/dev/tty.usbserial',
        dataBits: DataBits.Seven
      });
    });

    it('should set flow control', async () => {
      await serialPort.setFlowControl(FlowControl.Hardware);
      expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|set_flow_control', {
        path: '/dev/tty.usbserial',
        flowControl: FlowControl.Hardware
      });
    });

    it('should set parity', async () => {
      await serialPort.setParity(Parity.Even);
      expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|set_parity', {
        path: '/dev/tty.usbserial',
        parity: Parity.Even
      });
    });

    it('should set stop bits', async () => {
      await serialPort.setStopBits(StopBits.Two);
      expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|set_stop_bits', {
        path: '/dev/tty.usbserial',
        stopBits: StopBits.Two
      });
    });

    it('should set timeout', async () => {
      await serialPort.setTimeout(500);
      expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|set_timeout', {
        path: '/dev/tty.usbserial',
        timeout: 500
      });
    });

    it('should set RTS', async () => {
      await serialPort.setRequestToSend(true);
      expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|write_request_to_send', {
        path: '/dev/tty.usbserial',
        level: true
      });
    });

    it('should set DTR', async () => {
      await serialPort.setDataTerminalReady(true);
      expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|write_data_terminal_ready', {
        path: '/dev/tty.usbserial',
        level: true
      });
    });

    it('should clear buffer', async () => {
      await serialPort.clearBuffer(ClearBuffer.All);
      expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|clear_buffer', {
        path: '/dev/tty.usbserial',
        bufferType: ClearBuffer.All
      });
    });

    it('should clear input buffer', async () => {
      await serialPort.clearBuffer(ClearBuffer.Input);
      expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|clear_buffer', {
        path: '/dev/tty.usbserial',
        bufferType: ClearBuffer.Input
      });
    });

    it('should clear output buffer', async () => {
      await serialPort.clearBuffer(ClearBuffer.Output);
      expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|clear_buffer', {
        path: '/dev/tty.usbserial',
        bufferType: ClearBuffer.Output
      });
    });
  });

  describe('Break Control', () => {
    beforeEach(() => {
      serialPort.isOpen = true;
    });

    it('should set break', async () => {
      await serialPort.setBreak();
      expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|set_break', {
        path: '/dev/tty.usbserial'
      });
    });

    it('should clear break', async () => {
      await serialPort.clearBreak();
      expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|clear_break', {
        path: '/dev/tty.usbserial'
      });
    });
  });

  describe('Buffer Operations', () => {
    beforeEach(() => {
      serialPort.isOpen = true;
    });

    it('should get bytes to read', async () => {
      mockInvoke.mockResolvedValueOnce(10);
      const bytes = await serialPort.bytesToRead();
      expect(bytes).toBe(10);
      expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|bytes_to_read', {
        path: '/dev/tty.usbserial'
      });
    });

    it('should get bytes to write', async () => {
      mockInvoke.mockResolvedValueOnce(5);
      const bytes = await serialPort.bytesToWrite();
      expect(bytes).toBe(5);
      expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|bytes_to_write', {
        path: '/dev/tty.usbserial'
      });
    });
  });

  describe('Port Configuration', () => {
    beforeEach(() => {
      serialPort.isOpen = true;
    });

    describe('change', () => {
      it('should change port path and reopen port', async () => {
        const newPath = '/dev/tty.usbserial2';
        mockInvoke.mockResolvedValueOnce(undefined); // cancel_read
        mockInvoke.mockResolvedValueOnce(undefined); // close
        mockInvoke.mockResolvedValueOnce(undefined); // open

        await serialPort.change({ path: newPath });
        expect(serialPort.options.path).toBe(newPath);
        expect(mockInvoke).toHaveBeenCalledTimes(3); // cancel_read + close + open
        expect(mockInvoke).toHaveBeenNthCalledWith(1, 'plugin:serialplugin|cancel_read', {
          path: '/dev/tty.usbserial'
        });
        expect(mockInvoke).toHaveBeenNthCalledWith(2, 'plugin:serialplugin|close', {
          path: '/dev/tty.usbserial'
        });
        expect(mockInvoke).toHaveBeenNthCalledWith(3, 'plugin:serialplugin|open', {
          path: newPath,
          baudRate: 9600,
          dataBits: DataBits.Eight,
          flowControl: FlowControl.None,
          parity: Parity.None,
          stopBits: StopBits.One,
          timeout: 1000
        });
      });

      it('should change baud rate and reopen port', async () => {
        const newBaudRate = 115200;
        mockInvoke.mockResolvedValueOnce(undefined); // cancel_read
        mockInvoke.mockResolvedValueOnce(undefined); // close
        mockInvoke.mockResolvedValueOnce(undefined); // open

        await serialPort.change({ baudRate: newBaudRate });
        expect(serialPort.options.baudRate).toBe(newBaudRate);
        expect(mockInvoke).toHaveBeenCalledTimes(3); // cancel_read + close + open
      });

      it('should handle errors during change', async () => {
        serialPort.isOpen = true; // Port must be open for change to work
        mockInvoke.mockRejectedValueOnce(new Error('Cancel read error')); // cancelRead
        mockInvoke.mockResolvedValueOnce(undefined); // close
        mockInvoke.mockResolvedValueOnce(undefined); // open
        
        // change() should handle errors internally
        await expect(serialPort.change({ path: '/dev/tty.usbserial2' })).resolves.toBeUndefined();
        expect(mockInvoke).toHaveBeenCalledTimes(3); // cancel_read + close + open
      });
    });
  });
}); 
