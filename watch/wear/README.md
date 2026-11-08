# GS26 Wear OS companion

Read-only Wear OS app for the UBSEDS GS phone app. It shows live data pages, an
auto-cycle page and a recovery compass. It talks only to the phone, over the
Wear Data Layer, and only sends `hello`, `activity` and `settings` messages.
The protocol and behaviour are specified in `../../docs/watch-companion.md`.

```
core/   pure Kotlin/JVM: wire models + parsing, geodesy, formatting,
        settings editing, compass and link-state models (unit tested)
app/    Wear OS app (Compose for Wear OS, minSdk 30)
```

## Requirements

* JDK 17 or 21 (Gradle 8.14 does not run on newer JDKs)
* Android SDK with `platforms;android-36` and `build-tools;36.0.0`
  (`ANDROID_HOME` set, or `sdk.dir` in an untracked `local.properties`)

## Build and test

```sh
./gradlew :core:test :app:assembleDebug
./gradlew :app:lintDebug          # optional
```

`:core:test` checks the shared vectors in `../test-vectors.json` (the same file
the Rust and Swift tests use) plus protocol, settings, compass and link-loss
cases. The debug APK is written to `app/build/outputs/apk/debug/app-debug.apk`.

## Install on a watch

1. Enable developer options and ADB debugging (Wi-Fi debugging on most
   watches) on the watch, then pair: `adb pair <ip>:<pair-port>` and
   `adb connect <ip>:<port>`.
2. `adb -s <watch> install -r app/build/outputs/apk/debug/app-debug.apk`
3. Open the GS app on the paired phone. The watch shows
   "Open the GS app on your phone" until the first snapshot arrives.

## Data Layer pairing rules

The Data Layer only connects a watch app to a phone app that has the **same
applicationId and the same signing certificate**.

* The applicationId defaults to `com.UBSEDS.GS26` (the `identifier` in
  `Dioxus.toml`). Confirm it against the generated phone project's
  `app/build.gradle.kts` under `target/dx/`
  (`grep -r applicationId $(find target/dx -name build.gradle.kts)` from `ui/`).
  If it differs, build with
  `./gradlew :app:assembleDebug -Pgs26.applicationId=<phone id>` or change
  `gs26.applicationId` in `gradle.properties`.
* Debug builds of both apps on the same machine share
  `~/.android/debug.keystore`, so they match. For release, sign the watch APK
  with the phone app's release key (add a `signingConfigs` block or sign with
  `apksigner`).

Capabilities: the watch advertises `gs26_watch` (`res/values/wear.xml`); it
sends to nodes advertising `gs26_phone`.

## Behaviour notes

* Snapshots: DataItem `/gs26/snapshot` (`json`, `seq`). The last one and its
  receive time are kept in SharedPreferences, so the app opens with the last
  data and shows "Phone link lost · last data …" after `link_timeout_s`.
* Settings edits apply at once, show "Syncing with phone…" until a snapshot
  echoes `updated_ms` ≥ the edit, and are sent both as a message on
  `/gs26/watch` and as DataItem `/gs26/watch_settings` (`json` = the same
  `{"v":1,"type":"settings",...}` message, plus `updated_ms`).
* GPS and the rotation-vector sensor run only while the compass page is on
  screen and the app is resumed. Location permission is requested the first
  time the compass page is shown.
