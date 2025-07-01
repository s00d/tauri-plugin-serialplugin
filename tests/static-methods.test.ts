import { SerialPort, PortInfo } from '../guest-js';
import { setupTestMocks } from './test-utils';

describe('SerialPort Static Methods', () => {
  let mockInvoke: ReturnType<typeof setupTestMocks>['mockInvoke'];

  beforeEach(() => {
    const mocks = setupTestMocks();
    mockInvoke = mocks.mockInvoke;
  });

  afterEach(async () => {
    await SerialPort.closeAll();
  });

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
