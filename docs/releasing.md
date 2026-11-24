# Releasing the frontend

Pushing a tag `v<version>b<build>` runs `.github/workflows/release.yml`. It
builds the release apps, signs Android and macOS with the production
identities from repository secrets, and publishes a GitHub Release named
`Version <version> Build <build>`, like the earlier releases.

| Job | Runner | Assets |
|---|---|---|
| `unsigned` linux | ubuntu-24.04 | `UBSEDS.GS_x86_64.AppImage`, `.flatpak`, `.pkg.tar.gz`, `UBSEDS.GS_amd64.deb`, `UBSEDS.GS_x86_64.rpm` |
| `unsigned` linux-arm64 | ubuntu-24.04-arm | `UBSEDS.GS_aarch64.AppImage`, `.flatpak`, `.pkg.tar.gz`, `UBSEDS.GS_arm64.deb`, `UBSEDS.GS_aarch64.rpm` |
| `unsigned` windows | windows-2022 | `UBSEDS.GroundStation.Installer.exe` (NSIS, no Authenticode signature) |
| `android` android | ubuntu-24.04 | `SEDS.GS.apk` (arm64-v8a, production-signed) |
| `android` android-armv7 | ubuntu-24.04 | `SEDS.GS_armv7.apk` (armeabi-v7a, production-signed) |
| `macos` macos | macos-15 | `UBSEDS.GS.dmg` (Apple silicon, Developer ID, notarized and stapled) |
| `macos` macos-intel | macos-15-intel | `UBSEDS.GS_x86_64.dmg` (Intel, Developer ID, notarized and stapled) |
| `publish` | ubuntu-24.04 | all of the above, plus `SHA256SUMS` |

The names are the ones GitHub showed for v0.3.1b35 (GitHub turns spaces into
dots). There are a few differences:

- The Arch package is now `.pkg.tar.gz`; b35 had `.pkg.tar.zst`. build.py
  builds it with `PKGEXT=.pkg.tar.gz`.
- New assets: deb, rpm, Linux arm64, the armv7 APK and the Intel DMG.

iOS, watchOS and Wear OS are not part of the release.

Every job runs the same `scripts/ci/frontend.py bootstrap` and `build.py`
code paths as the build-only CI (`platforms.yml`), with warnings fatal:

- Linux and Windows build with `build.py --target <t> --release --no-sign`.
- Android and macOS build with `--release --final`. That is the same
  finalisation used locally:
  - Android: `_configure_android_signing` sets up the Gradle signing config,
    then `_finalize_android_release` runs apksigner.
  - macOS: `sign_macos_app_and_dmg` signs the app and DMG (hardened runtime,
    secure timestamp, `platform/macos/entitlements.plist`), then
    `notarize_macos` runs `notarytool submit --wait`, `stapler staple`,
    `stapler validate` and spctl.

## Signing material: only in the release workflow, only from secrets

- `platforms.yml` (push/PR CI) never sees a secret. Android CI APKs are signed
  with a fresh throwaway key and labelled not-for-distribution. macOS CI builds
  are unsigned.
- `tests/test_ci_release.py` enforces where secrets can appear:
  - only in `release.yml`;
  - only in the `android` and `macos` jobs;
  - only in step `env:`, never in `run:` text;
  - in each signing job, the last step is `release-cleanup` with `if: always()`.
- The signing files are decoded into `$RUNNER_TEMP/seds-release-signing/`
  (mode 0700, files 0600), never inside the checkout:
  - the keystore;
  - the `.p12` and `.p8`, which are deleted right after they are imported;
  - a temporary keychain with a random password.
- Only `release-assets/<target>/` is uploaded. Staging copies exactly the
  expected files and refuses keystores, `.p12`, `.p8` and keychains.
- Build logs (`ci-results/`) of the signing jobs are never uploaded.
- If the secrets are missing, the job fails with a message naming them. There
  is no fallback to a development key or to an unsigned DMG.

## 1. Bump the version

1. `Cargo.toml` `[package].version` (semver, each part at most 99) and
   `Dioxus.toml` `[application].build` (0–999). The Android `versionCode` is
   `major*10^7 + minor*10^5 + patch*10^3 + build`, so it must increase from one
   release to the next. For example, 0.3.1 build 36 is 301036.
2. Optional: write release notes in `docs/release-notes/v<version>b<build>.md`.
   Without that file the release uses GitHub's `--generate-notes`.
3. Merge to `main` (earlier releases were tagged from `main`).

The `version` job refuses a tag that does not equal
`v<Cargo version>b<Dioxus build>`.

## 2. Add the secrets (once)

Set the secrets with `gh secret set` from the machine that holds the key
material. The Android keystore and the Developer ID certificate are on the
Mac. `R=Rylan-Meilutis/Seds-Ground-Station-Frontend`.

### Android

| Secret | Value |
|---|---|
| `ANDROID_UPLOAD_KEYSTORE_B64` | base64 of the keystore (JKS or PKCS12) |
| `ANDROID_KEYSTORE_PASSWORD` | store password |
| `ANDROID_KEY_PASSWORD` | optional; key password, defaults to the store password |
| `ANDROID_KEY_ALIAS` | optional; defaults to `upload` |

```sh
# macOS (the keystore lives on the Mac)
base64 -i ~/keys/groundstation-upload.jks | gh secret set ANDROID_UPLOAD_KEYSTORE_B64 -R "$R"
# Linux
base64 -w0 ~/keys/groundstation-upload.jks | gh secret set ANDROID_UPLOAD_KEYSTORE_B64 -R "$R"

gh secret set ANDROID_KEYSTORE_PASSWORD -R "$R"   # prompts; nothing in shell history
gh secret set ANDROID_KEY_PASSWORD -R "$R"        # only if it differs from the store password
gh secret set ANDROID_KEY_ALIAS -R "$R"           # only if not "upload"
```

The macOS keychain items `gs26-android-keystore-pass` and
`gs26-android-key-pass` are where build.py reads these passwords locally.

Recommended: pin the expected certificate as a repository **variable**. It is
not secret. The release then refuses to publish an APK signed by any other key:

```sh
gh variable set ANDROID_EXPECTED_CERT_SHA256 -R "$R" --body "<SHA-256 of the chosen certificate>"
```

### macOS

| Secret | Value |
|---|---|
| `MACOS_DEVELOPER_ID_P12_B64` | base64 of the Developer ID Application certificate and private key, exported as `.p12` |
| `MACOS_DEVELOPER_ID_P12_PASSWORD` | the `.p12` export password |
| `MACOS_SIGNING_IDENTITY` | optional; e.g. `Developer ID Application: Name (TEAMID)`. Derived from the `.p12` when it holds exactly one Developer ID Application identity |
| `APPLE_NOTARY_KEY_P8_B64` | preferred notarization: base64 of the App Store Connect API key `.p8` |
| `APPLE_NOTARY_KEY_ID` | the API key ID |
| `APPLE_NOTARY_ISSUER_ID` | the API issuer ID (UUID) |
| `APPLE_ID`, `APPLE_APP_SPECIFIC_PASSWORD`, `APPLE_TEAM_ID` | fallback notarization, used only if the three `APPLE_NOTARY_*` secrets are unset |

On the Mac:

1. Find the identity:
   `security find-identity -v -p codesigning`. Look for the line
   `"Developer ID Application: … (TEAMID)"`.
2. Export the certificate and its private key. In Keychain Access → login →
   My Certificates, expand "Developer ID Application: …", select the
   certificate **and** its key, then right-click → Export 2 items… →
   `developer-id.p12` with a strong password.
3. Set the secrets:

   ```sh
   base64 -i developer-id.p12 | gh secret set MACOS_DEVELOPER_ID_P12_B64 -R "$R"
   gh secret set MACOS_DEVELOPER_ID_P12_PASSWORD -R "$R"
   gh secret set MACOS_SIGNING_IDENTITY -R "$R" --body "Developer ID Application: … (TEAMID)"
   rm -P developer-id.p12
   ```

4. Create a notarization API key:
   1. Go to App Store Connect → Users and Access → Integrations → App Store
      Connect API → Team Keys → "+".
   2. Name it, e.g. "GS notarization", with the **Developer** role.
   3. Download `AuthKey_<KEYID>.p8`. It can only be downloaded once.
   4. Note the Key ID and the Issuer ID shown above the list.

   ```sh
   base64 -i AuthKey_<KEYID>.p8 | gh secret set APPLE_NOTARY_KEY_P8_B64 -R "$R"
   gh secret set APPLE_NOTARY_KEY_ID -R "$R" --body "<KEYID>"
   gh secret set APPLE_NOTARY_ISSUER_ID -R "$R" --body "<issuer uuid>"
   ```

   Keep the `.p8` somewhere safe, or delete it; it can be revoked and
   reissued.

In the job, `release-macos-signing` does the following:

1. Creates a keychain in `$RUNNER_TEMP` with a random password and imports the
   `.p12` into it.
2. Allows codesign to use the key.
3. Adds the keychain to the search list.
4. Stores a `notarytool` profile in that keychain.

build.py then uses `MACOS_SIGNING_IDENTITY`, `NOTARY_PROFILE` and
`NOTARY_KEYCHAIN`, so no credential is on a command line during the build.
The job summary records:

- the identity;
- the notarization submission ID and status;
- the `codesign --verify --deep --strict`, `stapler validate` and
  `spctl -a -vvv -t install` results.

Until the Apple secrets exist, run the workflow manually with "skip macOS"
(below). A tag push without them fails the macOS job and publishes nothing.

## 3. Cut the release

```sh
git checkout main && git pull
git tag -a v0.3.1b36 -m "Version 0.3.1 Build 36"
git push origin v0.3.1b36
```

- Re-run, or release a tag without macOS, from Actions → Release → Run
  workflow, with `tag` set to e.g. `v0.3.1b36` and "skip macOS" as needed. Or:
  `gh workflow run release.yml -R "$R" -f tag=v0.3.1b36 -f skip_macos=true`.
- A new release is created as a draft, every asset is uploaded, and only then
  is it published. Re-running for an existing release updates the title (and
  the notes, if the notes file exists) and replaces the assets with
  `--clobber`.

## 4. Check the APK certificate

```sh
gh release download v0.3.1b36 -R "$R" -p SEDS.GS.apk -D /tmp/apk
"$ANDROID_HOME"/build-tools/36.0.0/apksigner verify --print-certs /tmp/apk/SEDS.GS.apk
# certificate of the keystore, to compare:
keytool -list -v -keystore ~/keys/groundstation-upload.jks -alias upload | grep SHA256
```

`keytool -printcert -jarfile` only reads v1 (JAR) signatures. Our APKs are
v2/v3 only (minSdk 24), so it reports "Not a signed jar file"; use apksigner.

### Previous release (v0.3.1b35) certificate

`SEDS.GS.apk` of v0.3.1b35, read from its APK Signature Scheme v2 and v3
blocks:

- DN: `CN=Android Debug, O=Android, C=US`, serial 1, valid 2026-03-12 to 2056-03-04
- SHA-256: `41:BE:92:37:28:08:D8:9E:C0:01:3D:89:B5:2E:20:3D:8D:5A:A6:FD:0A:A7:F8:40:38:39:29:BC:D7:87:8F:0E`
- SHA-1: `2D:00:C6:60:5A:80:8E:F6:06:7E:F5:11:62:63:58:F7:22:DF:4B:3C`

**b35 was signed with a debug key, not with `groundstation-upload.jks`.** It
is the Android debug keystore of the machine that built it
(`~/.android/debug.keystore` on the Mac). bundletool signs the universal APK
with that keystore when it is given no key.

Android only installs an update signed by the same certificate. So the first
release signed with the upload key cannot be installed over b35: phones must
uninstall b35 first, which loses the app's local data. Choose one:

- **Production upload key (recommended going forward).** Use
  `groundstation-upload.jks` for the four secrets. Each phone uninstalls b35
  once. Pin the upload certificate's SHA-256 in `ANDROID_EXPECTED_CERT_SHA256`.
- **Keep update continuity with b35.** Put the Mac's `~/.android/debug.keystore`
  in the secrets instead (alias `androiddebugkey`, store and key password
  `android`). Set `ANDROID_EXPECTED_CERT_SHA256` to the b35 SHA-256 above.
  This keeps a publicly known password on the distribution key.

After the first production-signed release, record its certificate here and in
`ANDROID_EXPECTED_CERT_SHA256`. Every later release must match it.
