# Publishing

Maintainer checklist. Everything registry-side uses **pnpm** (not mixed with `npm` CLI).

## 1. Bump version

Pick **one**:

```bash
pnpm release:patch
pnpm release:minor
pnpm release:major
```

Each runs release-it: bumps `package.json` + `Cargo.toml`, updates `CHANGELOG.md`, commits, tags `v*`, pushes, creates a GitHub Release.

## 2. Auth

npm registry sessions expire often. Check and fix with pnpm only:

```bash
pnpm whoami
# if that fails:
pnpm login
```

crates.io — either interactive login **or** a token env var:

```bash
cargo login
# writes ~/.cargo/credentials.toml

# CI / non-interactive alternative:
export CARGO_REGISTRY_TOKEN=...   # from https://crates.io/settings/tokens
```

`scripts/publish.sh` accepts either the credentials file or `CARGO_REGISTRY_TOKEN`.

## 3. Publish

```bash
pnpm release:publish
```

What it does, in order:

1. `pnpm whoami` — abort if logged out  
2. cargo auth (`CARGO_REGISTRY_TOKEN` or `~/.cargo/credentials.toml`)  
3. `pnpm install --frozen-lockfile`  
4. build + pack surface check  
5. `pnpm publish` → `tauri-plugin-serialplugin-api`  
6. `cargo publish` → `tauri-plugin-serialplugin`

Dry-run:

```bash
./scripts/publish.sh --dry-run
```

Surface check only:

```bash
pnpm publish:check
```

## android-usb-serial

If you changed that crate’s version, publish it **before** the plugin crate:

```bash
cargo publish -p android-usb-serial
# wait for crates.io index, then:
pnpm release:publish
```

## Packages

| Registry  | Package                         | Version file                                      |
|-----------|---------------------------------|---------------------------------------------------|
| npm       | `tauri-plugin-serialplugin-api` | `package.json`                                    |
| crates.io | `tauri-plugin-serialplugin`     | `Cargo.toml` (synced on bump)                     |
| crates.io | `android-usb-serial`            | `crates/android-usb-serial/Cargo.toml` (manual) |
