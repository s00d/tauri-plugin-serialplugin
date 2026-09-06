# Contributing

Thanks for helping improve **tauri-plugin-serialplugin**.

## Prerequisites

- Rust stable (see `rust-version` in `Cargo.toml`)
- Node 20+ and [pnpm](https://pnpm.io) 9.15.x (see `packageManager` in `package.json`)
- For Android / Robolectric: **JDK 17**

## Quick start

```bash
pnpm install
pnpm check && pnpm test && pnpm build
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Faster local gate (no Android cross / Robolectric):

```bash
./scripts/ci-fast.sh
```

Playground app:

```bash
pnpm playground
```

## Project layout

| Path | Role |
|------|------|
| `src/` | Rust plugin (desktop + Android) |
| `guest-js/` | TypeScript API published as `tauri-plugin-serialplugin-api` |
| `permissions/` | Tauri ACL permission TOMLs |
| `android/` | Kotlin / Gradle Android module |
| `crates/android-usb-serial/` | Pure-Rust USB serial drivers |
| `examples/serialport-test/` | Demo / playground |
| `tests/` | Jest suites for guest-js |

## Pull requests

1. Keep changes focused; prefer small PRs.
2. Match existing style; run fmt / clippy / tests before pushing.
3. Update `CHANGELOG.md` for user-visible changes (or rely on `pnpm release:patch` / `release:minor` / `release:major`, which write it via release-it).
4. Do **not** add new top-level CI workflows unless an existing job cannot cover the need.
5. Public API changes (Rust or JS) need a clear migration note for the next semver bump.

### Semver reminders

- **Patch**: bugfixes, docs, internal refactors with no public API break.
- **Minor**: additive API (new commands / options) that stays backward compatible.
- **Major**: removed / renamed JS or Rust exports, ACL identifier renames, behavior breaks.

Deprecated Rust items (e.g. `PortBackend`) stay until the next major.

## Publishing (maintainers)

Full checklist: **[PUBLISHING.md](./PUBLISHING.md)**.

Bump (pick one):

```bash
pnpm release:patch
pnpm release:minor
pnpm release:major
```

Auth + publish:

```bash
pnpm whoami
# if logged out:
pnpm login
cargo login

pnpm release:publish
```

Dry-run: `./scripts/publish.sh --dry-run`  
Surface check: `pnpm publish:check`

`release-it` bumps `package.json` + `Cargo.toml` (via `scripts/sync-cargo-version.cjs`), writes `CHANGELOG.md`, tags, and opens a GitHub Release.  
`scripts/publish.sh` publishes with **pnpm** first, then `cargo publish`.

## Security

See [SECURITY.md](./SECURITY.md). Do not file public issues for vulnerabilities.

## License

Contributions are dual-licensed under MIT and Apache-2.0, same as the repository.
