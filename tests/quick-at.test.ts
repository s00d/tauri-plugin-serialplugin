import {
  buildQuickAtSend,
  canSendQuickAt,
} from '../examples/serialport-test/src/quickAt';

describe('quick AT helpers', () => {
  it('allows send only when connected and not busy', () => {
    expect(canSendQuickAt(false)).toBe(false);
    expect(canSendQuickAt(true, true)).toBe(false);
    expect(canSendQuickAt(true)).toBe(true);
    expect(canSendQuickAt(true, false)).toBe(true);
  });

  it('builds an AT send payload with line ending and echo', () => {
    expect(buildQuickAtSend('ATI', 'crlf', false)).toEqual({
      text: 'ATI',
      mode: 'at',
      lineEnding: 'crlf',
      localEcho: false,
    });
  });
});
