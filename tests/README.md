# Tests

## Jest (guest-js)

```bash
pnpm test
```

Key suites:

- `watch-lifecycle.test.ts` — Channel + `watch` / `unwatch`
- `port-operations.test.ts`, `encoding-operations.test.ts`
- `auto-reconnect.test.ts` — reconnect on `watch` disconnect

Mocks: `@tauri-apps/api/core` (`invoke`, `Channel`) in `tests/setup.ts`.

## Rust

```bash
cargo nextest run
```

- `watch_registry_test`, `invoke_contract_test`, `desktop_api_test::watch_sends_disconnect_when_pty_peer_closed`

## Android

```bash
cd android && ./gradlew test
```
