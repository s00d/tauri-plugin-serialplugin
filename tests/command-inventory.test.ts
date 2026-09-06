import * as fs from 'fs';
import * as path from 'path';

/** Keep in sync with `HANDLER_COMMANDS` in `src/tests/command_inventory_test.rs`. */
const HANDLER_COMMANDS = [
  'available_ports',
  'managed_ports',
  'cancel_read',
  'close',
  'close_all',
  'force_close',
  'open',
  'capabilities',
  'watch',
  'unwatch',
  'watch_ports',
  'unwatch_ports',
  'read',
  'read_binary',
  'write',
  'write_binary',
  'set_baud_rate',
  'set_data_bits',
  'set_flow_control',
  'set_parity',
  'set_stop_bits',
  'set_timeout',
  'write_request_to_send',
  'write_data_terminal_ready',
  'read_clear_to_send',
  'read_data_set_ready',
  'read_ring_indicator',
  'read_carrier_detect',
  'bytes_to_read',
  'bytes_to_write',
  'clear_buffer',
  'set_break',
  'clear_break',
  'set_log_level',
  'get_log_level',
  'exchange',
  'exchange_binary',
  'cancel_exchange',
  'at',
  'at_phases',
  'send_sms_pdu',
  'configure_at_session',
  'enable_mux',
  'open_mux_channel',
  'disable_mux',
] as const;

function guestJsInvokeCommands(source: string): Set<string> {
  const needle = 'plugin:serialplugin|';
  const out = new Set<string>();
  let rest = source;
  while (true) {
    const i = rest.indexOf(needle);
    if (i < 0) break;
    const after = rest.slice(i + needle.length);
    const end = after.search(/[^A-Za-z0-9_]/);
    const name = end < 0 ? after : after.slice(0, end);
    out.add(name);
    rest = after.slice(name.length);
  }
  return out;
}

describe('command inventory (guest-js)', () => {
  it('extracts plugin:serialplugin|NAME invokes matching handler list', () => {
    const file = path.join(__dirname, '..', 'guest-js', 'serial-port.ts');
    const source = fs.readFileSync(file, 'utf8');
    const found = guestJsInvokeCommands(source);
    const expected = new Set(HANDLER_COMMANDS);
    expect([...found].sort()).toEqual([...expected].sort());
    expect(found.size).toBe(HANDLER_COMMANDS.length);
  });
});
