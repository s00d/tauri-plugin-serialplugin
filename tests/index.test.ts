import { SerialPort, DataBits, FlowControl, Parity, StopBits, ClearBuffer, PortInfo } from '../guest-js';
import { invoke } from "@tauri-apps/api/core";
import { listen } from '@tauri-apps/api/event';

// Mock Tauri API
jest.mock('@tauri-apps/api/core', () => ({
  invoke: jest.fn(),
}));

jest.mock('@tauri-apps/api/event', () => ({
  listen: jest.fn(),
}));

// Types for mocks
type MockInvoke = jest.Mock<Promise<any>, [string, any?]>;
type MockListen = jest.Mock<Promise<() => void>, [string, (event: any) => void]>;

// Clear all intervals after each test
afterEach(() => {
  jest.clearAllTimers();
});

// Clear all intervals after all tests
afterAll(() => {
  jest.clearAllTimers();
});

describe('SerialPort', () => {
  let mockInvoke: MockInvoke;
  let mockListen: MockListen;
  let mockUnlisten: jest.Mock;
  let serialPort: SerialPort;

  beforeEach(() => {
    // Clear all mocks before each test
    mockInvoke = invoke as MockInvoke;
    mockListen = listen as MockListen;
    mockUnlisten = jest.fn();
    mockListen.mockResolvedValue(mockUnlisten);
    jest.clearAllMocks();

    // Create new SerialPort instance for each test
    serialPort = new SerialPort({
      path: '/dev/tty.usbserial',
      baudRate: 9600,
      dataBits: DataBits.Eight,
      flowControl: FlowControl.None,
      parity: Parity.None,
      stopBits: StopBits.One,
      timeout: 1000
    });
  });

  afterEach(async () => {
    // Clear all test ports and listeners
    await SerialPort.closeAll();
  });

  describe('Static Methods', () => {
    describe('available_ports', () => {
      it('should return list of available ports', async () => {
        const mockPorts: { [key: string]: PortInfo } = {
          '/dev/tty.usbserial': {
            path: '/dev/tty.usbserial',
            manufacturer: 'FTDI',
            pid: '6001',
            product: 'USB Serial',
            serial_number: 'A12345',
            type: 'USB',
            vid: '0403'
          }
        };

        mockInvoke.mockResolvedValueOnce(mockPorts);

        const ports = await SerialPort.available_ports();
        expect(ports).toEqual(mockPorts);
        expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|available_ports');
      });

      it('should handle errors', async () => {
        const error = new Error('Failed to get ports');
        mockInvoke.mockRejectedValueOnce(error);

        await expect(SerialPort.available_ports()).rejects.toThrow('Failed to get ports');
      });
    });

    describe('available_ports_direct', () => {
      it('should return list of available ports using direct method', async () => {
        const mockPorts: { [key: string]: PortInfo } = {
          '/dev/tty.usbserial': {
            path: '/dev/tty.usbserial',
            manufacturer: 'FTDI',
            pid: '6001',
            product: 'USB Serial',
            serial_number: 'A12345',
            type: 'USB',
            vid: '0403'
          }
        };

        mockInvoke.mockResolvedValueOnce(mockPorts);

        const ports = await SerialPort.available_ports_direct();
        expect(ports).toEqual(mockPorts);
        expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|available_ports_direct');
      });
    });

    describe('managed_ports', () => {
      it('should return list of managed ports', async () => {
        const mockPorts = ['/dev/tty.usbserial'];
        mockInvoke.mockResolvedValueOnce(mockPorts);

        const ports = await SerialPort.managed_ports();
        expect(ports).toEqual(mockPorts);
        expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|managed_ports');
      });
    });

    describe('forceClose', () => {
      it('should force close a port', async () => {
        mockInvoke.mockResolvedValueOnce(undefined);

        await SerialPort.forceClose('/dev/tty.usbserial');
        expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|force_close', {
          path: '/dev/tty.usbserial'
        });
      });
    });

    describe('closeAll', () => {
      it('should close all ports', async () => {
        mockInvoke.mockResolvedValueOnce(undefined);

        await SerialPort.closeAll();
        expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|close_all');
      });
    });
  });

  describe('Instance Methods', () => {
    describe('constructor', () => {
      it('should create instance with default values', () => {
        const port = new SerialPort({
          path: '/dev/tty.usbserial',
          baudRate: 9600
        });

        expect(port.isOpen).toBe(false);
        expect(port.encoding).toBe('utf-8');
        expect(port.options.dataBits).toBe(DataBits.Eight);
        expect(port.options.flowControl).toBe(FlowControl.None);
        expect(port.options.parity).toBe(Parity.None);
        expect(port.options.stopBits).toBe(StopBits.One);
        expect(port.options.timeout).toBe(200);
        expect(port.size).toBe(1024);
      });

      it('should create instance with custom values', () => {
        const port = new SerialPort({
          path: '/dev/tty.usbserial',
          baudRate: 115200,
          encoding: 'ascii',
          dataBits: DataBits.Seven,
          flowControl: FlowControl.Hardware,
          parity: Parity.Even,
          stopBits: StopBits.Two,
          timeout: 500,
          size: 2048
        });

        expect(port.options.baudRate).toBe(115200);
        expect(port.encoding).toBe('ascii');
        expect(port.options.dataBits).toBe(DataBits.Seven);
        expect(port.options.flowControl).toBe(FlowControl.Hardware);
        expect(port.options.parity).toBe(Parity.Even);
        expect(port.options.stopBits).toBe(StopBits.Two);
        expect(port.options.timeout).toBe(500);
        expect(port.size).toBe(2048);
      });
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

    describe('listen', () => {
      beforeEach(() => {
        serialPort.isOpen = false;
      });

      it('should start listening successfully', async () => {
        serialPort.isOpen = true;
        const callback = jest.fn();
        mockListen.mockResolvedValueOnce(mockUnlisten);

        await serialPort.listen(callback);
        expect(serialPort.unListen).toBe(mockUnlisten);
        expect(mockListen).toHaveBeenCalled();
      });

      it('should throw error if port is not open', async () => {
        const callback = jest.fn();
        await expect(serialPort.listen(callback)).rejects.toEqual('Port is not open');
      });
    });

    describe('cancelListen', () => {
      it('should cancel listening successfully', async () => {
        serialPort.unListen = mockUnlisten;
        await serialPort.cancelListen();
        expect(mockUnlisten).toHaveBeenCalled();
        expect(serialPort.unListen).toBeUndefined();
      });
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

    describe('Break Control', () => {
      beforeEach(() => {
        serialPort.isOpen = true;
      });

      it('should set break', async () => {
        mockInvoke.mockResolvedValueOnce(true);
        const result = await serialPort.setBreak();
        expect(result).toBe(true);
        expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|set_break', {
          path: '/dev/tty.usbserial'
        });
      });

      it('should clear break', async () => {
        mockInvoke.mockResolvedValueOnce(true);
        const result = await serialPort.clearBreak();
        expect(result).toBe(true);
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
        const result = await serialPort.bytesToRead();
        expect(result).toBe(10);
        expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|bytes_to_read', {
          path: '/dev/tty.usbserial'
        });
      });

      it('should get bytes to write', async () => {
        mockInvoke.mockResolvedValueOnce(5);
        const result = await serialPort.bytesToWrite();
        expect(result).toBe(5);
        expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|bytes_to_write', {
          path: '/dev/tty.usbserial'
        });
      });

      it('should handle buffer operations with custom size', async () => {
        mockInvoke.mockResolvedValueOnce(2048);
        const result = await serialPort.bytesToRead();
        expect(result).toBe(2048);
        expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|bytes_to_read', {
          path: '/dev/tty.usbserial'
        });

        mockInvoke.mockResolvedValueOnce(1024);
        const writeResult = await serialPort.bytesToWrite();
        expect(writeResult).toBe(1024);
        expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|bytes_to_write', {
          path: '/dev/tty.usbserial'
        });
      });
    });

    describe('Binary Operations', () => {
      beforeEach(() => {
        serialPort.isOpen = true;
      });

      it('should write binary data', async () => {
        const data = new Uint8Array([1, 2, 3, 4, 5]);
        mockInvoke.mockResolvedValueOnce(5);
        const bytesWritten = await serialPort.writeBinary(data);
        expect(bytesWritten).toBe(5);
        expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|write_binary', {
          path: '/dev/tty.usbserial',
          value: Array.from(data)
        });
      });

      it('should read binary data', async () => {
        const mockData = new Uint8Array([1, 2, 3, 4, 5]);
        mockInvoke.mockResolvedValueOnce(Array.from(mockData));
        const data = await serialPort.readBinary();
        expect(data).toEqual(mockData);
        expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|read_binary', {
          path: '/dev/tty.usbserial',
          timeout: 1000,
          size: 1024
        });
      });
    });
  });

  describe('Error Handling', () => {
    let mockInvoke: MockInvoke;
    let mockListen: MockListen;
    let mockUnlisten: jest.Mock;
    let serialPort: SerialPort;

    beforeEach(() => {
      // Create new mocks for each test
      mockInvoke = jest.fn();
      mockListen = jest.fn();
      mockUnlisten = jest.fn();
      
      // Replace global mocks
      (invoke as any) = mockInvoke;
      (listen as any) = mockListen;
      mockListen.mockResolvedValue(mockUnlisten);

      // Create new SerialPort instance for each test
      serialPort = new SerialPort({
        path: '/dev/tty.usbserial',
        baudRate: 9600,
        dataBits: DataBits.Eight,
        flowControl: FlowControl.None,
        parity: Parity.None,
        stopBits: StopBits.One,
        timeout: 1000
      });

      // By default all commands are successful
      mockInvoke.mockResolvedValue(undefined);
    });

    afterEach(async () => {
      // Clear all mocks and state
      jest.clearAllMocks();
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

        // Check multiple recovery cycles
        for (let i = 0; i < 3; i++) {
          serialPort.isOpen = false; // Simulate port closure
          await expect(serialPort.write('test')).rejects.toEqual(`serial port ${serialPort.options.path} not opened!`);
          expect(callCount).toBe(i * 2); // invoke is not called because port is closed
          expect(serialPort.isOpen).toBe(false);
          
          await expect(serialPort.open()).resolves.toBeUndefined();
          expect(callCount).toBe(i * 2 + 1);
          expect(serialPort.isOpen).toBe(true);
          
          await expect(serialPort.write('test')).resolves.toBe(5);
          expect(callCount).toBe(i * 2 + 2);
          expect(serialPort.isOpen).toBe(true);
        }

        expect(mockInvoke).toHaveBeenCalledTimes(6);
      });

      it('should handle complex error patterns', async () => {
        mockInvoke.mockImplementation(() => {
          callCount++;
          switch (callCount) {
            case 1:
              return Promise.resolve(undefined); // open
            case 2:
            case 3:
            case 4:
              return Promise.resolve(5); // write
            case 5:
              return Promise.reject(new Error('Device busy')); // write
            case 6:
              return Promise.resolve(undefined); // open after Device busy error
            default:
              return Promise.resolve(undefined);
          }
        });

        // Check complex error pattern
        serialPort.isOpen = false;
        await expect(serialPort.write('test')).rejects.toEqual(`serial port ${serialPort.options.path} not opened!`);
        expect(callCount).toBe(0); // invoke is not called because port is closed
        expect(serialPort.isOpen).toBe(false);
        
        await expect(serialPort.write('test')).rejects.toEqual(`serial port ${serialPort.options.path} not opened!`);
        expect(callCount).toBe(0); // invoke is not called because port is closed
        expect(serialPort.isOpen).toBe(false);
        
        await expect(serialPort.open()).resolves.toBeUndefined();
        expect(callCount).toBe(1);
        expect(serialPort.isOpen).toBe(true);
        
        await expect(serialPort.write('test')).resolves.toBe(5);
        expect(callCount).toBe(2);
        expect(serialPort.isOpen).toBe(true);
        
        await expect(serialPort.write('test')).resolves.toBe(5);
        expect(callCount).toBe(3);
        expect(serialPort.isOpen).toBe(true);
        
        await expect(serialPort.write('test')).resolves.toBe(5);
        expect(callCount).toBe(4);
        expect(serialPort.isOpen).toBe(true);
        
        // Check Device busy error during write
        await expect(serialPort.write('test')).rejects.toThrow('Device busy');
        expect(callCount).toBe(5);
        expect(serialPort.isOpen).toBe(true);
        
        // Close port after error
        serialPort.isOpen = false;
        await expect(serialPort.write('test')).rejects.toEqual(`serial port ${serialPort.options.path} not opened!`);
        expect(callCount).toBe(5); // invoke is not called because port is closed
        expect(serialPort.isOpen).toBe(false);
        
        // Open port again
        await expect(serialPort.open()).resolves.toBeUndefined();
        expect(callCount).toBe(6);
        expect(serialPort.isOpen).toBe(true);

        expect(mockInvoke).toHaveBeenCalledTimes(6);
      });

      it('should handle error with custom message', async () => {
        serialPort.isOpen = true;
        
        const customError = new Error('Custom error message');
        mockInvoke.mockImplementationOnce(() => Promise.reject(customError));

        await expect(serialPort.write('test')).rejects.toThrow('Custom error message');
        expect(mockInvoke).toHaveBeenCalledTimes(1);
      });
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
        const consoleSpy = jest.spyOn(console, 'error').mockImplementation();
        
        // Create mock that throws error on call
        const mockUnlistenError = jest.fn().mockImplementation(() => {
          throw new Error('Cancel listen error');
        });
        serialPort.unListen = mockUnlistenError;
        
        // cancelListen should handle errors internally and not throw
        await expect(serialPort.cancelListen()).resolves.toBeUndefined();
        expect(mockUnlistenError).toHaveBeenCalled();
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
        mockListen.mockImplementation((event, cb) => {
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
        await expect(serialPort.clearBuffer(ClearBuffer.All)).rejects.toThrow('Clear buffer error');
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

      it('should handle error in readBinary', async () => {
        serialPort.isOpen = true;
        mockInvoke.mockRejectedValueOnce(new Error('Read binary error'));
        await expect(serialPort.readBinary()).rejects.toThrow('Read binary error');
      });

      it('should handle error in writeBinary with invalid data type', async () => {
        serialPort.isOpen = true;
        await expect(serialPort.writeBinary(null as any)).rejects.toEqual(
          'value Argument type error! Expected type: string, Uint8Array, number[]'
        );
      });
    });
  });

  describe('Encoding Operations', () => {
    let mockInvoke: MockInvoke;
    let mockListen: MockListen;
    let mockUnlisten: jest.Mock;
    let serialPort: SerialPort;

    beforeEach(() => {
      // Create new mocks for each test
      mockInvoke = jest.fn();
      mockListen = jest.fn();
      mockUnlisten = jest.fn();
      
      // Replace global mocks
      (invoke as any) = mockInvoke;
      (listen as any) = mockListen;
      mockListen.mockResolvedValue(mockUnlisten);

      // Create new SerialPort instance for each test
      serialPort = new SerialPort({
        path: '/dev/tty.usbserial',
        baudRate: 9600,
        dataBits: DataBits.Eight,
        flowControl: FlowControl.None,
        parity: Parity.None,
        stopBits: StopBits.One,
        timeout: 1000
      });

      serialPort.isOpen = true;
    });

    afterEach(async () => {
      jest.clearAllMocks();
      await SerialPort.closeAll();
    });

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

  });

  describe('Concurrent Operations', () => {
    beforeEach(() => {
      serialPort.isOpen = true;
    });

    it('should handle multiple concurrent reads', async () => {
      const promises = Array(5).fill(null).map(async () => {
        mockInvoke.mockResolvedValueOnce('test data');
        return await serialPort.read();
      });

      const results = await Promise.all(promises);
      results.forEach(result => {
        expect(result).toBe('test data');
      });
    });

    it('should handle multiple concurrent writes', async () => {
      const promises = Array(5).fill(null).map(async (_, index) => {
        const data = `test data ${index}`;
        mockInvoke.mockResolvedValueOnce(data.length);
        return await serialPort.write(data);
      });

      const results = await Promise.all(promises);
      results.forEach((result, index) => {
        expect(result).toBe(`test data ${index}`.length);
      });
    });

    it('should handle mixed read/write operations', async () => {
      const operations = [
        async () => {
          mockInvoke.mockResolvedValueOnce(5);
          return await serialPort.write('write1');
        },
        async () => {
          mockInvoke.mockResolvedValueOnce('read1');
          return await serialPort.read();
        },
        async () => {
          mockInvoke.mockResolvedValueOnce(5);
          return await serialPort.write('write2');
        },
        async () => {
          mockInvoke.mockResolvedValueOnce('read2');
          return await serialPort.read();
        }
      ];

      const results = await Promise.all(operations.map(op => op()));
      expect(results).toEqual([5, 'read1', 5, 'read2']);
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
        mockListen.mockResolvedValueOnce(mockUnlisten); // disconnected

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
        expect(mockListen).toHaveBeenCalledTimes(1); // disconnected
      });

      it('should change baud rate and reopen port', async () => {
        const newBaudRate = 115200;
        mockInvoke.mockResolvedValueOnce(undefined); // cancel_read
        mockInvoke.mockResolvedValueOnce(undefined); // close
        mockInvoke.mockResolvedValueOnce(undefined); // open
        mockListen.mockResolvedValueOnce(mockUnlisten); // disconnected

        await serialPort.change({ baudRate: newBaudRate });
        expect(serialPort.options.baudRate).toBe(newBaudRate);
        expect(mockInvoke).toHaveBeenCalledTimes(3); // cancel_read + close + open
        expect(mockListen).toHaveBeenCalledTimes(1); // disconnected
      });

      it('should handle errors during change', async () => {
        serialPort.isOpen = true; // Port must be open for change to work
        mockInvoke.mockRejectedValueOnce(new Error('Cancel read error')); // cancelRead
        mockInvoke.mockResolvedValueOnce(undefined); // close
        mockInvoke.mockResolvedValueOnce(undefined); // open
        mockListen.mockResolvedValueOnce(mockUnlisten); // disconnected
        
        // change() should handle errors internally
        await expect(serialPort.change({ path: '/dev/tty.usbserial2' })).resolves.toBeUndefined();
        expect(mockInvoke).toHaveBeenCalledTimes(3); // cancel_read + close + open
        expect(mockListen).toHaveBeenCalledTimes(1); // disconnected
      });
    });
  });

  describe('Port Operations', () => {
    describe('close', () => {
      it('should handle errors during cancelRead', async () => {
        serialPort.isOpen = true;
        mockInvoke.mockRejectedValueOnce(new Error('Cancel read error'));
        // close() should handle cancelRead errors internally
        await expect(serialPort.close()).resolves.toBeUndefined();
      });

      it('should handle errors during port close', async () => {
        serialPort.isOpen = true;
        mockInvoke.mockResolvedValueOnce(undefined); // cancelRead
        mockInvoke.mockRejectedValueOnce(new Error('Close error'));
        // close() should handle port close errors internally
        await expect(serialPort.close()).resolves.toBeUndefined();
      });
    });

    describe('listen', () => {
      it('should handle errors in callback with binary data', async () => {
        serialPort.isOpen = true;
        const consoleSpy = jest.spyOn(console, 'error').mockImplementation();
        const callback = jest.fn().mockImplementation(() => {
          throw new Error('Binary callback error');
        });

        mockListen.mockImplementationOnce((event, cb) => {
          cb({ payload: { data: [1, 2, 3, 4, 5] } });
          return Promise.resolve(mockUnlisten);
        });

        await serialPort.listen(callback, false);
        expect(callback).toHaveBeenCalled();
        expect(consoleSpy).toHaveBeenCalledWith(expect.any(Error));
        consoleSpy.mockRestore();
      });

      it('should handle errors in callback with text data', async () => {
        serialPort.isOpen = true;
        const consoleSpy = jest.spyOn(console, 'error').mockImplementation();
        const callback = jest.fn().mockImplementation(() => {
          throw new Error('Text callback error');
        });

        mockListen.mockImplementationOnce((event, cb) => {
          cb({ payload: { data: [72, 101, 108, 108, 111] } }); // "Hello" in ASCII
          return Promise.resolve(mockUnlisten);
        });

        await serialPort.listen(callback, true);
        expect(callback).toHaveBeenCalled();
        expect(consoleSpy).toHaveBeenCalledWith(expect.any(Error));
        consoleSpy.mockRestore();
      });
    });

    describe('open', () => {
      it('should handle errors during disconnected event setup', async () => {
        const consoleSpy = jest.spyOn(console, 'error').mockImplementation();
        mockInvoke.mockResolvedValueOnce(undefined); // open
        mockListen.mockRejectedValueOnce(new Error('Disconnected event error'));

        await serialPort.open();
        expect(consoleSpy).toHaveBeenCalledWith(expect.any(Error));
        consoleSpy.mockRestore();
      });

      it('should handle errors during disconnected callback', async () => {
        const consoleSpy = jest.spyOn(console, 'error').mockImplementation();
        mockInvoke.mockResolvedValueOnce(undefined); // open
        mockListen.mockImplementationOnce((event, cb) => {
          cb({}); // Call callback with empty event object
          return Promise.resolve(mockUnlisten);
        });

        await serialPort.open();
        expect(consoleSpy).toHaveBeenCalledWith(expect.any(Error));
        consoleSpy.mockRestore();
      });
    });

    describe('writeBinary', () => {
      it('should handle invalid input type', async () => {
        serialPort.isOpen = true;
        await expect(serialPort.writeBinary('invalid' as any)).rejects.toEqual(
          'value Argument type error! Expected type: string, Uint8Array, number[]'
        );
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
    });
  });

  // Test for unregisterListener issue fix
  describe('unregisterListener error handling', () => {
    it('should handle unregisterListener errors gracefully', async () => {
      const serialPort = new SerialPort({ path: 'COM1', baudRate: 9600 });
      
      // Mock listen to return a function that throws an error
      const mockUnlisten = jest.fn().mockImplementation(() => {
        throw new Error('window.TAURI_EVENT_PLUGIN_INTERNALS.unregisterListener is undefined');
      });
      
      (listen as jest.Mock).mockResolvedValue(mockUnlisten);
      
      // Open port and set up listener
      await serialPort.open();
      await serialPort.startListening();
      await serialPort.listen(() => {});
      
      // This should not throw an error now
      await expect(serialPort.cancelListen()).resolves.toBeUndefined();
      
      // Close should also work without errors
      await expect(serialPort.close()).resolves.toBeUndefined();
    });

    it('should handle multiple listen/cancelListen cycles', async () => {
      const serialPort = new SerialPort({ path: 'COM1', baudRate: 9600 });
      
      const mockUnlisten = jest.fn();
      (listen as jest.Mock).mockResolvedValue(mockUnlisten);
      
      await serialPort.open();
      await serialPort.startListening();
      
      // Multiple listen/cancelListen cycles should work
      for (let i = 0; i < 5; i++) {
        await serialPort.listen(() => {});
        await serialPort.cancelListen();
      }
      
      await serialPort.close();
      
      // Should not have thrown any errors
      expect(mockUnlisten).toHaveBeenCalledTimes(5);
    });
  });
}); 
