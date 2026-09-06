# Publishing

Short guide for maintainers. Details: [CONTRIBUTING.md](./CONTRIBUTING.md).

## One-shot flow

```bash
# 1) Bump version, CHANGELOG, git tag, GitHub Release
pnpm release          # or: release:patch | release:minor | release:major

# 2) Auth (npm expires often — re-login before publish)
npm login             # or: pnpm login
cargo login           # once; token in ~/.cargo/credentials.toml

# 3) Publish registries (npm first, then crates.io)
pnpm release:publish
# dry-run: ./scripts/publish.sh --dry-run
```

## Order (important)

1. **Check npm auth** (`npm whoami`) — fail early if logged out  
2. **Check cargo credentials**  
3. **`pnpm install` + build + surface check**  
4. **`npm publish`** (`tauri-plugin-serialplugin-api`)  
5. **`cargo publish`** (`tauri-plugin-serialplugin`)

npm is first because npm sessions frequently invalidate; crates.io tokens are more stable. If cargo fails after npm, fix cargo and re-run only the cargo step (npm version is already taken).

## android-usb-serial

The plugin depends on `android-usb-serial` by **version** on crates.io. If you bumped that crate:

```bash
cargo publish -p android-usb-serial
# wait until crates.io indexes it, then:
cargo publish
```

## Checks

```bash
pnpm publish:check    # pack surface + Cargo.toml exclude guards
./scripts/publish.sh --dry-run
```

## Packages

| Registry | Name | Version source |
|----------|------|----------------|
| npm | `tauri-plugin-serialplugin-api` | `package.json` |
| crates.io | `tauri-plugin-serialplugin` | `Cargo.toml` (synced by release-it) |
| crates.io | `android-usb-serial` | `crates/android-usb-serial/Cargo.toml` (manual when changed) |
