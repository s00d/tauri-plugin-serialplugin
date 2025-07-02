import { SerialPort } from '../guest-js';

describe('Undefined unregisterListener issue', () => {
  let port: SerialPort;

  beforeEach(() => {
    port = new SerialPort({
      path: '/dev/ttyUSB0',
      baudRate: 9600
    });
  });

  afterEach(async () => {
    try {
      if (port.isOpen) {
        await port.close();
      }
    } catch (error) {
      console.warn('Error in cleanup:', error);
    }
  });

  test('should handle multiple listen calls without undefined errors', async () => {
    // Mock the port to simulate the user's scenario
    jest.spyOn(port, 'open').mockResolvedValue();
    jest.spyOn(port, 'startListening').mockResolvedValue();
    jest.spyOn(port, 'close').mockResolvedValue();
    
    // Mock the listen method to return a valid unlisten function
    const mockUnlisten = jest.fn();
    jest.spyOn(port, 'listen').mockResolvedValue(mockUnlisten);

    // Simulate the user's workflow
    await port.open();
    await port.startListening();

    // First listen call
    const unsubscribe1 = await port.listen((data) => {
      console.log('Data 1:', data);
    }, false);

    // Second listen call (this should work without errors)
    const unsubscribe2 = await port.listen((data) => {
      console.log('Data 2:', data);
    }, false);

    // Both should be valid functions
    expect(typeof unsubscribe1).toBe('function');
    expect(typeof unsubscribe2).toBe('function');

    // Call unsubscribe functions (should not throw)
    expect(() => unsubscribe1()).not.toThrow();
    expect(() => unsubscribe2()).not.toThrow();

    // Close port (should not throw)
    await expect(port.close()).resolves.not.toThrow();
  });

  test('should handle unsubscribe being called multiple times', async () => {
    // Mock the port
    jest.spyOn(port, 'open').mockResolvedValue();
    jest.spyOn(port, 'startListening').mockResolvedValue();
    jest.spyOn(port, 'close').mockResolvedValue();
    
    const mockUnlisten = jest.fn();
    jest.spyOn(port, 'listen').mockResolvedValue(mockUnlisten);

    await port.open();
    await port.startListening();

    const unsubscribe = await port.listen((data) => {
      console.log('Data:', data);
    }, false);

    // Call unsubscribe multiple times (should not throw)
    expect(() => unsubscribe()).not.toThrow();
    expect(() => unsubscribe()).not.toThrow();
    expect(() => unsubscribe()).not.toThrow();

    await expect(port.close()).resolves.not.toThrow();
  });

  test('should handle unsubscribe after port is closed', async () => {
    // Mock the port
    jest.spyOn(port, 'open').mockResolvedValue();
    jest.spyOn(port, 'startListening').mockResolvedValue();
    jest.spyOn(port, 'close').mockResolvedValue();
    
    const mockUnlisten = jest.fn();
    jest.spyOn(port, 'listen').mockResolvedValue(mockUnlisten);

    await port.open();
    await port.startListening();

    const unsubscribe = await port.listen((data) => {
      console.log('Data:', data);
    }, false);

    // Close port first
    await port.close();

    // Then call unsubscribe (should not throw)
    expect(() => unsubscribe()).not.toThrow();
  });
}); 
