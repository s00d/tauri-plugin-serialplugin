import { SerialPort } from '../guest-js';
import { setupTestMocks, createTestSerialPort } from './test-utils';

describe('SerialPort Concurrent Operations', () => {
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

    it('should handle concurrent binary operations', async () => {
      const promises = Array(3).fill(null).map(async (_, index) => {
        const data = new Uint8Array([index, index + 1, index + 2]);
        mockInvoke.mockResolvedValueOnce(data.length);
        return await serialPort.writeBinary(data);
      });

      const results = await Promise.all(promises);
      results.forEach((result, index) => {
        expect(result).toBe(3); // Each array has 3 elements
      });
    });

    it('should handle concurrent port settings', async () => {
      const operations = [
        async () => {
          mockInvoke.mockResolvedValueOnce(undefined);
          return await serialPort.setBaudRate(115200);
        },
        async () => {
          mockInvoke.mockResolvedValueOnce(undefined);
          return await serialPort.setDataBits('Seven' as any);
        },
        async () => {
          mockInvoke.mockResolvedValueOnce(undefined);
          return await serialPort.setParity('Even' as any);
        }
      ];

      await Promise.all(operations.map(op => op()));
      expect(mockInvoke).toHaveBeenCalledTimes(3);
    });

    it('should handle concurrent control signal operations', async () => {
      const operations = [
        async () => {
          mockInvoke.mockResolvedValueOnce(undefined);
          return await serialPort.setRequestToSend(true);
        },
        async () => {
          mockInvoke.mockResolvedValueOnce(undefined);
          return await serialPort.setDataTerminalReady(false);
        },
        async () => {
          mockInvoke.mockResolvedValueOnce(true);
          return await serialPort.readClearToSend();
        }
      ];

      const results = await Promise.all(operations.map(op => op()));
      expect(results).toEqual([undefined, undefined, true]);
      expect(mockInvoke).toHaveBeenCalledTimes(3);
    });
  });
}); 
