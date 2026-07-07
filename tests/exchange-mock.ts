import type { AtParseStatus, ExchangeMatch, ExchangeResponse } from '../guest-js';

export function mockExchangeResponse(
  text: string,
  overrides: { status?: AtParseStatus; matched?: ExchangeMatch } = {},
): ExchangeResponse {
  const raw = [...text].map((c) => c.charCodeAt(0));
  const lines = text
    .replace(/\r/g, '')
    .split('\n')
    .map((l) => l.trim())
    .filter(Boolean);
  const status = overrides.status ?? 'ok';
  const matched: ExchangeMatch =
    overrides.matched ?? (status === 'ok' ? 'ok' : status === 'error' ? 'error' : 'idle');
  const urcLines = lines.filter((l) => l.startsWith('+') && !l.startsWith('+CME') && !l.startsWith('+CMS'));
  const solicitedBody = lines.filter(
    (l) => l.startsWith('+') && !urcLines.includes(l) && l !== 'OK' && l !== 'ERROR',
  );
  return {
    raw,
    matched,
    lines,
    status,
    solicitedBody,
    urcLines,
  };
}

export function mockAtCommandResult(
  command: string,
  text: string,
  overrides: { status?: AtParseStatus; matched?: ExchangeMatch } = {},
) {
  const exchange = mockExchangeResponse(text, overrides);
  return {
    command,
    response: text,
    raw: exchange.raw,
    matched: exchange.matched,
    lines: exchange.lines,
    status: exchange.status,
    solicitedBody: exchange.solicitedBody,
    urcLines: exchange.urcLines,
    timedOut: false as const,
  };
}
