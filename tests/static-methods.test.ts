import { SerialPort, PortInfo } from '../guest-js';
import { Channel } from '@tauri-apps/api/core';
import { MockChannel } from './setup';
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
      expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|available_ports', {
        singlePortPerDevice: false,
      });
    });

    it('should pass singlePortPerDevice when requested', async () => {
      mockInvoke.mockResolvedValueOnce({});

      await SerialPort.available_ports({ singlePortPerDevice: true });

      expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|available_ports', {
        singlePortPerDevice: true,
      });
    });

    it('should handle errors', async () => {
      const error = new Error('Failed to get ports');
      mockInvoke.mockRejectedValueOnce(error);

      await expect(SerialPort.available_ports()).rejects.toThrow('Failed to get ports');
    });
  });

  describe('watchAvailablePorts', () => {
    it('subscribes with channel and returns handle', async () => {
      mockInvoke.mockResolvedValueOnce(42);

      const onSnapshot = jest.fn();
      const handle = await SerialPort.watchAvailablePorts({ onSnapshot });

      expect(handle.channelId).toBe(42);
      expect(mockInvoke).toHaveBeenCalledWith(
        'plugin:serialplugin|watch_ports',
        expect.objectContaining({
          options: {},
          channel: expect.any(Channel),
        }),
      );

      await handle.unwatch();
      expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|unwatch_ports', {
        channelId: 42,
      });
    });

    it('passes watch options and dispatches port list events', async () => {
      mockInvoke.mockResolvedValueOnce(1);

      const onSnapshot = jest.fn();
      const onAdded = jest.fn();
      const onRemoved = jest.fn();

      await SerialPort.watchAvailablePorts(
        { onSnapshot, onAdded, onRemoved },
        { singlePortPerDevice: true, pollIntervalMs: 1000 },
      );

      expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|watch_ports', {
        options: { singlePortPerDevice: true, pollIntervalMs: 1000 },
        channel: expect.any(Channel),
      });

      const channel = MockChannel.lastInstance!;
      channel.onmessage?.({
        kind: 'snapshot',
        ports: {
          '/dev/ttyUSB0': {
            path: '/dev/ttyUSB0',
            type: 'USB',
            manufacturer: 'Unknown',
            pid: 'Unknown',
            product: 'Unknown',
            serial_number: 'Unknown',
            vid: 'Unknown',
          },
        },
      });
      expect(onSnapshot).toHaveBeenCalledWith({
        '/dev/ttyUSB0': expect.objectContaining({ type: 'USB' }),
      });

      channel.onmessage?.({
        kind: 'added',
        path: '/dev/ttyUSB1',
        info: {
          path: '/dev/ttyUSB1',
          type: 'USB',
          manufacturer: 'Unknown',
          pid: 'Unknown',
          product: 'Unknown',
          serial_number: 'Unknown',
          vid: 'Unknown',
        },
      });
      expect(onAdded).toHaveBeenCalledWith('/dev/ttyUSB1', expect.objectContaining({ type: 'USB' }));

      channel.onmessage?.({ kind: 'removed', path: '/dev/ttyUSB0' });
      expect(onRemoved).toHaveBeenCalledWith('/dev/ttyUSB0');
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
