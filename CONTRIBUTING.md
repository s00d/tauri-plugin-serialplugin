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

See **[PUBLISHING.md](./PUBLISHING.md)** for the short checklist.

Bump + changelog + git tag + GitHub Release (does **not** publish registries):

```bash
pnpm release          # interactive / conventional bump
pnpm release:patch    # explicit patch
pnpm release:minor
pnpm release:major
```

Uses [release-it](https://github.com/release-it/release-it) + conventional changelog.
`scripts/sync-cargo-version.cjs` keeps root `Cargo.toml` version in sync with `package.json`.

Auth (npm often forces re-login) then publish — **npm first**, then crates.io:

```bash
npm login             # or: pnpm login
cargo login           # if credentials missing
pnpm release:publish
# dry-run:
./scripts/publish.sh --dry-run
```

Surface check only: `pnpm publish:check`

### Why this stack (not more CI workflows)

| Tool | Role here |
|------|-----------|
| **release-it** | bump, CHANGELOG, tag, GitHub release |
| **scripts/publish.sh** | auth checks → `npm publish` → `cargo publish` |
| release-plz / pubm | optional later for fully automated CI publish — not required |

Do **not** add a second publish workflow unless secrets + automation are intentionally enabled.

## Security

See [SECURITY.md](./SECURITY.md). Do not file public issues for vulnerabilities.

## License

Contributions are dual-licensed under MIT and Apache-2.0, same as the repository.
