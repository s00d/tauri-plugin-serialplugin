# Android (`serialport-test`)

```bash
cd examples/serialport-test
export JAVA_HOME="$(/usr/libexec/java_home -v 17)"
export ADB_SERIAL=10.0.59.85:37389   # свой из `adb devices`
```

## Wireless ADB

```bash
# npm i -g adb-wifi && adb-wifi
adb pair 10.0.59.85:PAIR_PORT
adb connect 10.0.59.85:DEBUG_PORT
adb devices

adb kill-server && adb start-server && adb connect 10.0.59.85:DEBUG_PORT
```

## Сборка → install → логи

```bash
pnpm tauri android build --debug

adb -s "$ADB_SERIAL" install -r \
  /Users/s00d/packeges/tauri-plugin-serialplugin/examples/serialport-test/src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk

adb -s "$ADB_SERIAL" shell am force-stop com.serialport.test
adb -s "$ADB_SERIAL" shell am start -n com.serialport.test/.MainActivity

adb -s "$ADB_SERIAL" logcat -c
adb -s "$ADB_SERIAL" logcat -v time -s UsbFdBridge SerialPlugin Console
```

Все Rust-логи плагина идут в **`SerialPlugin`** (не `RustStdout`).

| Тег / префикс | Что |
|---|---|
| `SerialPlugin: load` / `logcat sink ready` | старт |
| `UsbFdBridge: enumerateJson … json=` | Kotlin USB list + `interfaces[]` |
| `[SerialEnumerate]` | Rust ProbeTable → `device` или `device#N` |
| `openDeviceFd … (unclaimed for nusb)` | fd без claim |
| `[SerialOpen]` / `start_reader` | claim → line → reader после DTR |
| `[SerialWrite]` | запись после connect |
| `Console: [PortPicker]` | UI snapshot |

Kotlin **не** claim'ит интерфейсы — это nusb. Write открывает только bulk OUT (IN у reader).

## CH340 / отвал после Connect (EPROTO)

Симптом: `URB ep 82 status=-71` → `Device disconnected` → `enumerateJson: 0`.

**Изоляция железо vs софт** — тот же адаптер через [`examples/usb-driver-tester`](../usb-driver-tester/):

```bash
cd examples/usb-driver-tester
./gradlew installDebug
# Test на устройстве → смотри лог в UI (Share)
```

| Результат | Вывод |
|---|---|
| tester **тоже** падает на open/read | питание OTG, кабель, хаб с питанием |
| tester **стабилен**, serialport-test отваливается | gap в плагине (reader/timing) — смотри `[SerialOpen]` |
| оба живут после хаба с питанием | слабый USB-порт телефона |

Плагин: reader стартует **после** line/DTR; bulk IN in-flight = 2 (не 3). Перед повторным тестом: `am force-stop com.serialport.test`.

Package: `com.serialport.test`.
