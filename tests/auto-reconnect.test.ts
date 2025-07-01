import { SerialPort } from '../guest-js';
import { setupTestMocks, createTestSerialPort } from './test-utils';

describe('SerialPort Auto-Reconnect', () => {
  let serialPort: SerialPort;
  let mockInvoke: ReturnType<typeof setupTestMocks>['mockInvoke'];
  let mockListen: ReturnType<typeof setupTestMocks>['mockListen'];

  beforeEach(() => {
    const mocks = setupTestMocks();
    mockInvoke = mocks.mockInvoke;
    mockListen = mocks.mockListen;
    serialPort = createTestSerialPort();
  });

  afterEach(async () => {
    await SerialPort.closeAll();
  });

  it('should automatically reconnect after a disconnect event', async () => {
    // Open the port
    mockInvoke.mockResolvedValueOnce(undefined); // for open
    await serialPort.open();
    expect(serialPort.isOpen).toBe(true);

    // Prepare to capture the disconnect callback
    let disconnectCallback: ((event: any) => void) | undefined;
    mockListen.mockImplementationOnce(async (_event, cb) => {
      disconnectCallback = cb;
      return jest.fn();
    });

    // Enable auto-reconnect
    await serialPort.enableAutoReconnect();
    expect(serialPort.getAutoReconnectInfo().enabled).toBe(true);

    // Simulate disconnect (port closes)
    serialPort.isOpen = false;
    // Next open call will succeed (simulate successful reconnect)
    mockInvoke.mockResolvedValueOnce(undefined);

    // Emulate Tauri disconnect event
    expect(disconnectCallback).toBeDefined();
    await disconnectCallback!({});

    // Check that the port is open again (reconnected)
    expect(serialPort.isOpen).toBe(true);
  });
}); 
 