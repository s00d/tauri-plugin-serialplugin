import { SerialPort, DataBits, FlowControl, Parity, StopBits, ClearBuffer, PortInfo } from '../guest-js';
import { invoke } from "@tauri-apps/api/core";
import { listen } from '@tauri-apps/api/event';

// Мокаем Tauri API
jest.mock('@tauri-apps/api/core', () => ({
  invoke: jest.fn(),
}));

jest.mock('@tauri-apps/api/event', () => ({
  listen: jest.fn(),
}));

// Типы для моков
type MockInvoke = jest.Mock<Promise<any>, [string, any?]>;
type MockListen = jest.Mock<Promise<() => void>, [string, (event: any) => void]>;

// Очищаем все интервалы после каждого теста
afterEach(() => {
  jest.clearAllTimers();
});

// Очищаем все интервалы после всех тестов
afterAll(() => {
  jest.clearAllTimers();
});

describe('SerialPort', () => {
  let mockInvoke: MockInvoke;
  let mockListen: MockListen;
  let mockUnlisten: jest.Mock;

  beforeEach(() => {
    // Очищаем все моки перед каждым тестом
    mockInvoke = invoke as MockInvoke;
    mockListen = listen as MockListen;
    mockUnlisten = jest.fn();
    mockListen.mockResolvedValue(mockUnlisten);
    jest.clearAllMocks();
  });

  afterEach(async () => {
    // Очищаем все тестовые порты и слушатели
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
    let serialPort: SerialPort;

    beforeEach(() => {
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

        await expect(serialPort.close()).rejects.toThrow('Failed to close port');
        expect(serialPort.isOpen).toBe(true);
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
}); 