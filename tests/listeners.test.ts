import { SerialPort } from '../guest-js';
import { setupTestMocks, createTestSerialPort } from './test-utils';

describe('SerialPort Listeners', () => {
  let mockListen: ReturnType<typeof setupTestMocks>['mockListen'];
  let mockUnlisten: ReturnType<typeof setupTestMocks>['mockUnlisten'];
  let serialPort: SerialPort;

  beforeEach(() => {
    const mocks = setupTestMocks();
    mockListen = mocks.mockListen;
    mockUnlisten = mocks.mockUnlisten;
    serialPort = createTestSerialPort();
  });

  afterEach(async () => {
    await SerialPort.closeAll();
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
      const listenersInfo = serialPort.getListenersInfo();
      expect(listenersInfo.data).toBe(1);
      expect(listenersInfo.total).toBe(1);
      expect(mockListen).toHaveBeenCalled();
    });

    it('should throw error if port is not open', async () => {
      const callback = jest.fn();
      await expect(serialPort.listen(callback)).rejects.toEqual('Port is not open');
    });

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

  describe('cancelListen', () => {
    it('should cancel listening successfully', async () => {
      // First set up a listener
      serialPort.isOpen = true;
      const callback = jest.fn();
      mockListen.mockResolvedValueOnce(mockUnlisten);
      await serialPort.listen(callback);
      
      // Verify listener was added
      expect(serialPort.getListenersInfo().data).toBe(1);
      
      // Cancel listening
      await serialPort.cancelListen();
      expect(mockUnlisten).toHaveBeenCalled();
      
      // Verify listener was removed
      expect(serialPort.getListenersInfo().data).toBe(0);
      expect(serialPort.getListenersInfo().total).toBe(0);
    });

    it('should handle multiple listeners when cancelling', async () => {
      serialPort.isOpen = true;
      const callback1 = jest.fn();
      const callback2 = jest.fn();
      
      // Create multiple listeners by directly adding them to the listeners map
      const mockUnlisten1 = jest.fn();
      const mockUnlisten2 = jest.fn();
      
      mockListen.mockResolvedValueOnce(mockUnlisten1);
      mockListen.mockResolvedValueOnce(mockUnlisten2);
      
      // Simulate adding listeners without cancelling previous ones
      const originalCancelListen = serialPort.cancelListen.bind(serialPort);
      serialPort.cancelListen = jest.fn(); // Disable automatic cancellation
      
      await serialPort.listen(callback1);
      await serialPort.listen(callback2);
      
      // Restore original method
      serialPort.cancelListen = originalCancelListen;
      
      // Verify we have 2 listeners
      expect(serialPort.getListenersInfo().data).toBe(2);
      
      // Cancel listening
      await serialPort.cancelListen();
      expect(mockUnlisten1).toHaveBeenCalled();
      expect(mockUnlisten2).toHaveBeenCalled();
      
      // Verify all listeners were removed
      expect(serialPort.getListenersInfo().data).toBe(0);
      expect(serialPort.getListenersInfo().total).toBe(0);
    });
  });

  describe('cancelAllListeners', () => {
    it('should cancel all data listeners', async () => {
      serialPort.isOpen = true;
      const callback1 = jest.fn();
      const callback2 = jest.fn();
      
      mockListen.mockResolvedValue(mockUnlisten);
      await serialPort.listen(callback1);
      
      // Note: listen() calls cancelListen() first, so only one listener exists at a time
      expect(serialPort.getListenersInfo().data).toBe(1);
      
      await serialPort.listen(callback2);
      expect(serialPort.getListenersInfo().data).toBe(2); // Still only one, as previous was cancelled
      
      await serialPort.cancelAllListeners();
      
      expect(serialPort.getListenersInfo().data).toBe(0);
      expect(serialPort.getListenersInfo().total).toBe(0);
    });

    it('should handle multiple listeners with manual management', async () => {
      serialPort.isOpen = true;
      const callback1 = jest.fn();
      const callback2 = jest.fn();
      
      // Create multiple listeners by directly adding them to the listeners map
      // This simulates what would happen if we didn't call cancelListen() in listen()
      const mockUnlisten1 = jest.fn();
      const mockUnlisten2 = jest.fn();
      
      mockListen.mockResolvedValueOnce(mockUnlisten1);
      mockListen.mockResolvedValueOnce(mockUnlisten2);
      
      // Simulate adding listeners without cancelling previous ones
      const originalCancelListen = serialPort.cancelListen.bind(serialPort);
      serialPort.cancelListen = jest.fn(); // Disable automatic cancellation
      
      await serialPort.listen(callback1);
      await serialPort.listen(callback2);
      
      // Restore original method
      serialPort.cancelListen = originalCancelListen;
      
      // Now we should have 2 listeners
      expect(serialPort.getListenersInfo().data).toBe(2);
      
      await serialPort.cancelAllListeners();
      
      expect(serialPort.getListenersInfo().data).toBe(0);
      expect(serialPort.getListenersInfo().total).toBe(0);
    });

    it('should cancel both data and disconnect listeners', async () => {
      serialPort.isOpen = true;
      const dataCallback = jest.fn();
      const disconnectCallback = jest.fn();
      
      const mockUnlisten1 = jest.fn();
      const mockUnlisten2 = jest.fn();
      
      mockListen.mockResolvedValueOnce(mockUnlisten1);
      mockListen.mockResolvedValueOnce(mockUnlisten2);
      
      // Add data listener
      await serialPort.listen(dataCallback);
      
      // Add disconnect listener
      await serialPort.disconnected(disconnectCallback);
      
      // Verify we have both types of listeners
      const info = serialPort.getListenersInfo();
      expect(info.data).toBe(1);
      expect(info.disconnect).toBe(1);
      expect(info.total).toBe(2);
      
      // Cancel all listeners
      await serialPort.cancelAllListeners();
      
      // Verify all listeners were cancelled
      expect(mockUnlisten1).toHaveBeenCalled();
      expect(mockUnlisten2).toHaveBeenCalled();
      
      const finalInfo = serialPort.getListenersInfo();
      expect(finalInfo.data).toBe(0);
      expect(finalInfo.disconnect).toBe(0);
      expect(finalInfo.total).toBe(0);
    });
  });

  describe('getListenersInfo', () => {
    it('should return correct listener information', async () => {
      serialPort.isOpen = true;
      const callback = jest.fn();
      mockListen.mockResolvedValue(mockUnlisten);
      
      await serialPort.listen(callback);
      
      const info = serialPort.getListenersInfo();
      expect(info.total).toBe(1);
      expect(info.data).toBe(1);
      expect(info.disconnect).toBe(0);
      expect(info.ids).toHaveLength(1);
    });

    it('should return correct information for disconnect listeners', async () => {
      const callback = jest.fn();
      mockListen.mockResolvedValue(mockUnlisten);
      
      await serialPort.disconnected(callback);
      
      const info = serialPort.getListenersInfo();
      expect(info.total).toBe(1);
      expect(info.data).toBe(0);
      expect(info.disconnect).toBe(1);
      expect(info.ids).toHaveLength(1);
    });
  });

  describe('disconnected', () => {
    it('should set up disconnect listener successfully', async () => {
      const callback = jest.fn();
      
      await serialPort.disconnected(callback);
      
      expect(mockListen).toHaveBeenCalled();
      const callArgs = mockListen.mock.calls[0];
      expect(callArgs[0]).toContain('plugin-serialplugin-disconnected');
      
      // Verify that disconnect listener was added
      const info = serialPort.getListenersInfo();
      expect(info.disconnect).toBe(1);
      expect(info.total).toBe(1);
    });

    it('should handle error in disconnected event', async () => {
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
  });

  describe('unregisterListener error handling', () => {
    it('should handle unregisterListener errors gracefully', async () => {
      const serialPort = new SerialPort({ path: 'COM1', baudRate: 9600 });
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
      (mockListen as jest.Mock).mockResolvedValue(mockUnlisten);
      
      await serialPort.open();
      await serialPort.startListening();
      
      // Multiple listen/cancelListen cycles should work
      for (let i = 0; i < 5; i++) {
        await serialPort.listen(() => {});
        await serialPort.cancelListen();
      }
      
      // Don't call close() as it would trigger additional cancelListen calls
      // Just verify that all data listeners were properly managed
      const listenersInfo = serialPort.getListenersInfo();
      expect(listenersInfo.data).toBe(0); // No data listeners should remain
      // Note: disconnect listener from open() is now managed by listen(), so total is 1
      expect(listenersInfo.disconnect).toBe(1);
      expect(listenersInfo.total).toBe(1);
      
      // Should not have thrown any errors
      expect(mockUnlisten).toHaveBeenCalledTimes(5);
    });
  });
}); 
