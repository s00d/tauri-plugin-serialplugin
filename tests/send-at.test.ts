import { setupTestMocks, createTestSerialPort } from './test-utils';
import { mockAtCommandResult } from './exchange-mock';

describe('SerialPort sendAt', () => {
  beforeEach(() => {
    setupTestMocks();
  });

  it('invokes native at command', async () => {
    const { mockInvoke } = setupTestMocks();
    mockInvoke.mockImplementation((cmd: string, args?: { command?: string }) => {
      if (cmd === 'plugin:serialplugin|open') return Promise.resolve();
      if (cmd === 'plugin:serialplugin|at') {
        return Promise.resolve(mockAtCommandResult(args?.command ?? 'AT', 'OK\r\n'));
      }
      return Promise.resolve();
    });

    const port = createTestSerialPort();
    await port.open();
    const result = await port.sendAt('AT');
    expect(result.command).toBe('AT');
    expect(result.status).toBe('ok');
    expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|at', {
      path: '/dev/tty.usbserial',
      command: 'AT',
      options: null,
    });
  });

  it('configureAtSession on open when atSession in constructor', async () => {
    const { mockInvoke } = setupTestMocks();
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'plugin:serialplugin|open') return Promise.resolve();
      if (cmd === 'plugin:serialplugin|configure_at_session') return Promise.resolve();
      return Promise.resolve();
    });

    const port = createTestSerialPort({
      atSession: { expectOk: true, defaultTimeoutMs: 3000 },
    });
    await port.open();
    expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|configure_at_session', {
      path: '/dev/tty.usbserial',
      session: { expectOk: true, defaultTimeoutMs: 3000 },
    });
  });

  it('cancelAt invokes cancel_exchange', async () => {
    const { mockInvoke } = setupTestMocks();
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'plugin:serialplugin|open') return Promise.resolve();
      if (cmd === 'plugin:serialplugin|cancel_exchange') return Promise.resolve();
      return Promise.resolve();
    });

    const port = createTestSerialPort();
    await port.open();
    await port.cancelAt();
    expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|cancel_exchange', {
      path: '/dev/tty.usbserial',
    });
  });

  it('sendSmsPdu invokes native send_sms_pdu', async () => {
    const { mockInvoke } = setupTestMocks();
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'plugin:serialplugin|open') return Promise.resolve();
      if (cmd === 'plugin:serialplugin|send_sms_pdu') {
        return Promise.resolve([
          mockAtCommandResult('AT+CMGS=10', '>\r\n'),
          mockAtCommandResult('', 'OK\r\n'),
        ]);
      }
      return Promise.resolve();
    });

    const port = createTestSerialPort();
    await port.open();
    const results = await port.sendSmsPdu(10, new Uint8Array([0x01, 0x02]));
    expect(results).toHaveLength(2);
    expect(mockInvoke).toHaveBeenCalledWith(
      'plugin:serialplugin|send_sms_pdu',
      expect.objectContaining({ length: 10, pdu: [1, 2] }),
    );
  });

  it('sendAtPhases invokes native at_phases', async () => {
    const { mockInvoke } = setupTestMocks();
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'plugin:serialplugin|open') return Promise.resolve();
      if (cmd === 'plugin:serialplugin|at_phases') {
        return Promise.resolve([mockAtCommandResult('AT', 'OK\r\n')]);
      }
      return Promise.resolve();
    });

    const port = createTestSerialPort();
    await port.open();
    await port.sendAtPhases([{ write: 'AT' }]);
    expect(mockInvoke).toHaveBeenCalledWith('plugin:serialplugin|at_phases', {
      path: '/dev/tty.usbserial',
      phases: [{ write: 'AT' }],
    });
  });
});
