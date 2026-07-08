import { LogLevel, getLogLevel, setLogLevel } from '../guest-js/logger';
import { SerialPort } from '../guest-js';
import { setupTestMocks } from './test-utils';

describe('logger', () => {
  afterEach(() => {
    setLogLevel(LogLevel.Info);
  });

  it('getLogLevel reflects setLogLevel', () => {
    setLogLevel(LogLevel.None);
    expect(getLogLevel()).toBe(LogLevel.None);
    setLogLevel(LogLevel.Debug);
    expect(getLogLevel()).toBe(LogLevel.Debug);
  });
});

describe('SerialPort.setLogLevel', () => {
  it('syncs internal logger from invoke', async () => {
    const { mockInvoke } = setupTestMocks();
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'plugin:serialplugin|set_log_level') return Promise.resolve();
      if (cmd === 'plugin:serialplugin|get_log_level') return Promise.resolve('Warn');
      return Promise.resolve();
    });

    await SerialPort.setLogLevel(LogLevel.Warn);
    expect(getLogLevel()).toBe(LogLevel.Warn);
  });
});
