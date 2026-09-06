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
3. Update `CHANGELOG.md` for user-visible changes (or rely on `pnpm release` / standard-version).
4. Do **not** add new top-level CI workflows unless an existing job cannot cover the need.
5. Public API changes (Rust or JS) need a clear migration note for the next semver bump.

### Semver reminders

- **Patch**: bugfixes, docs, internal refactors with no public API break.
- **Minor**: additive API (new commands / options) that stays backward compatible.
- **Major**: removed / renamed JS or Rust exports, ACL identifier renames, behavior breaks.

Deprecated Rust items (e.g. `PortBackend`) stay until the next major.

## Publishing (maintainers)

```bash
# bump version (Cargo.toml + package.json + CHANGELOG)
pnpm release

# build, pack checks, cargo publish, then npm publish
pnpm run release:publish
```

`scripts/publish.sh` publishes **crates.io first**, then npm, so a failed Rust publish does not leave a lone npm version.

Dry-run / surface check only:

```bash
pnpm run publish:check
```

## Security

See [SECURITY.md](./SECURITY.md). Do not file public issues for vulnerabilities.

## License

Contributions are dual-licensed under MIT and Apache-2.0, same as the repository.
