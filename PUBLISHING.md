# Publishing

Released from GitHub Actions: nothing is published from a laptop and no crates.io or npm
token is stored anywhere.

| Registry  | Package                         | Version                                              |
|-----------|---------------------------------|------------------------------------------------------|
| npm       | `tauri-plugin-serialplugin-api` | the release version                                  |
| crates.io | `tauri-plugin-serialplugin`     | the release version                                  |
| crates.io | `serialplugin-core`, `android-usb-serial`, `serial-codec`, `serialplugin-testing` | their own, bumped by hand  |

## Release

1. Write the notes under `## Unreleased` at the top of `CHANGELOG.md`.
2. If `serialplugin-core`, `android-usb-serial`, `serial-codec` or `serialplugin-testing` changed, bump its version in
   `crates/<name>/Cargo.toml` and the `version` requirement of every workspace crate that
   depends on it.
3. Push and wait for green CI on `main`.
4. Actions -> **CI** -> **Run workflow** -> `release`: the version (e.g. `4.2.3`, no `v`). The
   `release` job:
   - stops unless CI is green on `main` HEAD, or on an earlier commit with only `docs/` and
     `*.md` files changed since;
   - `scripts/release.mjs prep`: sets the version (Cargo.toml, package.json, lockfiles), renames
     `## Unreleased`, stops when it is empty or a changed crate was not bumped;
   - checks the package contents (`pnpm publish:check`), runs `cargo semver-checks` against
     crates.io and a publish dry run;
   - commits `chore(release): X.Y.Z` to `main` and tags `vX.Y.Z`;
   - publishes the crates that are not on crates.io yet, in dependency order, then the npm
     package, both with trusted publishing;
   - creates the GitHub release from the changelog section and rebuilds the docs site.

   If it fails after tagging (network, crates.io, npm), run it again with the same version: it
   publishes only what is missing.

## Setup (once)

1. **crates.io**, for each crate: page -> Settings -> Trusted Publishing -> Add -> GitHub.
   Owner `s00d`, repository `tauri-plugin-serialplugin`, workflow `ci.yml`, environment `crates-io`.
   - <https://crates.io/crates/tauri-plugin-serialplugin/settings>
   - <https://crates.io/crates/serialplugin-core/settings>
   - <https://crates.io/crates/android-usb-serial/settings>
   - <https://crates.io/crates/serial-codec/settings>
   - <https://crates.io/crates/serialplugin-testing/settings>
2. **npm**: <https://www.npmjs.com/package/tauri-plugin-serialplugin-api/access> -> Trusted
   Publisher -> GitHub Actions. User `s00d`, repository `tauri-plugin-serialplugin`, workflow
   filename `ci.yml`, environment `crates-io`.
3. **GitHub** (optional): <https://github.com/s00d/tauri-plugin-serialplugin/settings/environments>
   -> environment `crates-io` (created on the first run) -> required reviewers.

The environment must match the `environment:` of the `release` job, otherwise the token
exchange is refused. A new crate cannot be created through trusted publishing: publish a
`0.0.0` placeholder (an empty `lib.rs`) by hand once, then add its publisher. Not the real
version: the release would find it on crates.io with files changed since the last tag, and
refuse it.
