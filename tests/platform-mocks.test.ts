import { SerialPort } from '../guest-js';
import { setupTestMocks } from './test-utils';

describe('Platform event name normalization (mocked)', () => {
  let mockListen: ReturnType<typeof setupTestMocks>['mockListen'];

  const cases: Array<{ platform: string; path: string; expectedEvent: string }> = [
    {
      platform: 'windows',
      path: 'COM3',
      expectedEvent: 'plugin-serialplugin-disconnected-COM3',
    },
    {
      platform: 'linux',
      path: '/dev/ttyUSB0',
      expectedEvent: 'plugin-serialplugin-disconnected--dev-ttyUSB0',
    },
    {
      platform: 'macos',
      path: '/dev/tty.usbserial-1420',
      expectedEvent: 'plugin-serialplugin-disconnected--dev-tty-usbserial-1420',
    },
    {
      platform: 'android',
      path: '/dev/bus/usb/001/002',
      expectedEvent: 'plugin-serialplugin-disconnected--dev-bus-usb-001-002',
    },
  ];

  beforeEach(() => {
    ({ mockListen } = setupTestMocks());
  });

  it.each(cases)(
    'registers disconnected listener for $platform path',
    async ({ path, expectedEvent }) => {
      const port = new SerialPort({ path, baudRate: 9600 });
      const callback = jest.fn();

      await port.disconnected(callback);

      expect(mockListen).toHaveBeenCalledWith(expectedEvent, expect.any(Function));
      expect(port.getListenersInfo().disconnect).toBe(1);
    },
  );
});
