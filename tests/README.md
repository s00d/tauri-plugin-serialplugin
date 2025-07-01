# Tests for tauri-plugin-serialplugin

This directory contains tests for the tauri-plugin-serialplugin, organized into logical modules for better structure and maintainability.

## Test Structure

### `test-utils.ts`
Common utilities for tests, including:
- Mocks for Tauri API (`invoke`, `listen`, `once`)
- Functions for creating test SerialPort instances
- Global test setup and cleanup

### `static-methods.test.ts`
Tests for static methods of the SerialPort class:
- `available_ports()` - getting list of available ports
- `available_ports_direct()` - direct method for getting ports
- `managed_ports()` - getting managed ports
- `forceClose()` - force closing a port
- `closeAll()` - closing all ports

### `constructor.test.ts`
Tests for the SerialPort constructor:
- Creating instance with default values
- Creating instance with custom values

### `port-operations.test.ts`
Tests for basic port operations:
- `open()` - opening a port
- `close()` - closing a port
- `write()` - writing data
- `read()` - reading data
- `readBinary()` - reading binary data
- `writeBinary()` - writing binary data
- `startListening()` / `stopListening()` - listening management

### `listeners.test.ts`
Tests for the event listener system:
- `listen()` - setting up data listeners
- `cancelListen()` - canceling data listeners
- `cancelAllListeners()` - canceling all listeners
- `disconnected()` - port disconnect listener
- `getListenersInfo()` - listener information
- Handling unregisterListener errors

### `port-settings.test.ts`
Tests for port settings:
- `setBaudRate()` - setting baud rate
- `setDataBits()` - setting data bits
- `setFlowControl()` - setting flow control
- `setParity()` - setting parity
- `setStopBits()` - setting stop bits
- `setTimeout()` - setting timeout
- `clearBuffer()` - clearing buffer
- `setBreak()` / `clearBreak()` - break signal management
- `change()` - changing port configuration

### `control-signals.test.ts`
Tests for control signals:
- `readClearToSend()` - reading CTS
- `readDataSetReady()` - reading DSR
- `readRingIndicator()` - reading RI
- `readCarrierDetect()` - reading CD
- `writeRequestToSend()` - writing RTS
- `writeDataTerminalReady()` - writing DTR

### `error-handling.test.ts`
Tests for error handling:
- Single errors in various methods
- Sequential errors
- Mixed operations with recovery
- Error handling in specific methods

### `edge-cases.test.ts`
Tests for edge cases:
- Empty parameters
- Already opened/closed ports
- Invalid data types
- Errors in listeners
- Errors in various operations

### `concurrent-operations.test.ts`
Tests for concurrent operations:
- Multiple simultaneous reads
- Multiple simultaneous writes
- Mixed read/write operations
- Concurrent binary operations
- Concurrent port settings

### `encoding-operations.test.ts`
Tests for encoding operations:
- Working with various encodings (UTF-8, ASCII)
- Encoding error handling
- Fallback mechanisms in listeners
- Binary data in listeners

### `auto-reconnect.test.ts`
Tests for auto-reconnect functionality:
- `enableAutoReconnect()` - enabling auto-reconnect
- `disableAutoReconnect()` - disabling auto-reconnect
- `getAutoReconnectInfo()` - getting auto-reconnect status
- `manualReconnect()` - manual reconnection
- Automatic reconnection on disconnect events
- Reconnection with custom settings (interval, max attempts)
- Infinite reconnection attempts
- Reconnection callback handling

## Running Tests

```bash
# Run all tests
npm test

# Run specific test file
npm test -- tests/port-operations.test.ts

# Run tests with coverage
npm test -- --coverage

# Run tests in watch mode
npm test -- --watch
```

## Testing Features

1. **Tauri API Mocks**: All Tauri API calls are mocked for test isolation
2. **State Cleanup**: All ports are closed after each test
3. **Error Handling**: Tests verify both successful operations and error handling
4. **Concurrency**: Tests verify work with multiple simultaneous operations
5. **Encodings**: Tests cover various encoding scenarios
6. **Auto-Reconnect**: Tests verify automatic reconnection functionality
7. **Listener Management**: Tests verify proper listener lifecycle management

## Adding New Tests

When adding new tests:
1. Determine which logical module the test belongs to
2. Add the test to the appropriate file
3. Use common utilities from `test-utils.ts`
4. Follow existing naming and structure patterns
5. Ensure the test covers both successful and error scenarios
6. For auto-reconnect tests, use the disconnect event simulation pattern

## Test Utilities

### `setupTestMocks()`
Sets up mocks for Tauri API functions:
- `mockInvoke` - mocks the `invoke` function
- `mockListen` - mocks the `listen` function  
- `mockOnce` - mocks the `once` function
- `mockUnlisten` - mocks the unlisten function returned by `listen`

### `createTestSerialPort()`
Creates a test SerialPort instance with default configuration:
- Path: `/dev/tty.usbserial`
- Baud rate: 9600
- Data bits: Eight
- Flow control: None
- Parity: None
- Stop bits: One
- Timeout: 1000ms

## Auto-Reconnect Testing

Auto-reconnect tests use a special pattern to simulate disconnect events:

```typescript
// Capture the disconnect callback
let disconnectCallback: ((event: any) => void) | undefined;
mockListen.mockImplementationOnce(async (_event, cb) => {
  disconnectCallback = cb;
  return jest.fn();
});

// Enable auto-reconnect
await serialPort.enableAutoReconnect();

// Simulate disconnect event
serialPort.isOpen = false;
await disconnectCallback!({});

// Verify reconnection
expect(serialPort.isOpen).toBe(true);
```

This pattern allows testing the automatic reconnection functionality without requiring actual hardware disconnection. 
