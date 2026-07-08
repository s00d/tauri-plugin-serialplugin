# Golden fixture recorder archive

The one-time JVM golden recorder lived under `android/src/test/kotlin/.../golden/` and
depended on vendored `android/usbserial/` (usb-serial-for-android). That tree was removed
after Java fixtures were frozen under `crates/android-usb-serial/tests/fixtures/`.

**Current workflow:** regenerate Rust-only fixtures and verify parity:

```bash
cargo run -p android-usb-serial --features fake-transport --bin golden_record
cargo test -p android-usb-serial --features fake-transport --test golden_parity
./scripts/verify-android-usb-migration.sh
```

To recover the historical Java recorder for a one-shot re-record, check out a git commit
from before usbserial removal, e.g. `git checkout <commit> -- android/usbserial android/src/test/kotlin/app/tauri/serialplugin/golden/`.
