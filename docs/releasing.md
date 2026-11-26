# Releasing the frontend

Pushing a tag `v<version>b<build>` runs `.github/workflows/release.yml`. It
builds the release apps, signs Android and macOS with the production
identities from repository secrets, and publishes a GitHub Release named
`Version <version> Build <build>`, like the earlier releases. When their
secrets exist it also uploads the iOS app to **TestFlight** (optionally into
named beta groups) and an Android App Bundle to a **Google Play testing
track**: internal by default, or alpha (closed) / beta (open). Nothing ever
goes to a public App Store or Play production release; see
[Testing tracks and groups](#testing-tracks-and-groups).

The run refuses to sign anything unless it comes from a protected ref and the
tag's commit is on `main` or `dev` (section 0).

| Job | Runner | Assets |
|---|---|---|
| `unsigned` linux | ubuntu-24.04 | `UBSEDS.GS_x86_64.AppImage`, `.flatpak`, `.pkg.tar.gz`, `UBSEDS.GS_amd64.deb`, `UBSEDS.GS_x86_64.rpm` |
| `unsigned` linux-arm64 | ubuntu-24.04-arm | `UBSEDS.GS_aarch64.AppImage`, `.flatpak`, `.pkg.tar.gz`, `UBSEDS.GS_arm64.deb`, `UBSEDS.GS_aarch64.rpm` |
| `unsigned` windows | windows-2022 | `UBSEDS.GroundStation.Installer.exe` (NSIS, no Authenticode signature) |
| `android` android | ubuntu-24.04 | `SEDS.GS.apk` (arm64-v8a, production-signed) |
| `android` android-armv7 | ubuntu-24.04 | `SEDS.GS_armv7.apk` (armeabi-v7a, production-signed) |
| `macos` macos | xcode-27 (Xcode 27.2) | `UBSEDS.GS.dmg` (Apple silicon, Developer ID, notarized and stapled) |
| `macos` macos-intel | macos-15-intel | `UBSEDS.GS_x86_64.dmg` (Intel, Developer ID, notarized and stapled) |
| `android-launch` | ubuntu-24.04 (KVM) | no asset: builds the x86_64 release APK (CI development key, no secrets) and launches it on an API 36 emulator; `publish`, `dry-run` and `play` need it to pass |
| `publish` | ubuntu-24.04 | all of the above, plus `SHA256SUMS` (not in a dry run) |
| `dry-run` | ubuntu-24.04 | manual dry runs only: checks the signed set, publishes nothing |
| `testflight` | xcode-27 (Xcode 27.2) | `UBSEDS GS.ipa` (App Store signed) uploaded to App Store Connect / TestFlight, then added to `TESTFLIGHT_GROUPS` if set; not a GitHub asset |
| `play` | ubuntu-24.04 | `SEDS GS.aab` (arm64-v8a, upload-key signed) uploaded to the `PLAY_TRACK` testing track (default **internal**); not a GitHub asset |

The names are the ones GitHub showed for v0.3.1b35 (GitHub turns spaces into
dots). There are a few differences:

- The Arch package is now `.pkg.tar.gz`; b35 had `.pkg.tar.zst`. build.py
  builds it with `PKGEXT=.pkg.tar.gz`.
- New assets: deb, rpm, Linux arm64, the armv7 APK and the Intel DMG.

watchOS and Wear OS are not part of the release. iOS goes only to TestFlight.

Store minimums the release checks:

- **App Store Connect:** the iOS 27 SDK (ITMS-90725; Xcode 27, a hard error
  from April 2027) and a deployment target of iOS 15.0 or later (ITMS-90068).
  `release-store-verify` refuses an IPA built with an older SDK, or whose
  executable or `MinimumOSVersion` is below 15.0. GitHub's `xcode-27` image
  is Apple silicon only, so the Intel DMG still builds with Xcode 26 on
  macos-15-intel; Developer ID notarisation sets no SDK minimum. On the
  macOS 27 image the Apple silicon DMG is made with `diskutil image` (APFS
  volume), since `hdiutil create` is deprecated there; the Intel DMG stays
  HFS+ from `hdiutil`.
- **Google Play:** target API 36, and 16 KB page support for every 64-bit
  native library (NDK r28c plus `-z max-page-size=16384`). The release APK
  and the Play AAB both go through `scripts/check_android_alignment.py
  --release`, which also requires stripped native libraries (no `.debug_*`
  sections) and, in the AAB, their symbol tables under
  `BUNDLE-METADATA/com.android.tools.build.debugsymbols/` so Play can
  symbolicate native crashes. The manifest sets
  `android:enableOnBackInvokedCallback="false"`, so under target 36 the
  back button still reaches the WebView as before (no predictive back).

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

## 0. Protected refs (once)

The `version` job runs `frontend.py release-guard` before anything else. It
refuses the run unless all of these hold:

- it is a tag push of `refs/tags/<tag>`, or a `workflow_dispatch` started from
  `main` or `dev`;
- GitHub reports that ref as protected (`GITHUB_REF_PROTECTED`, which is
  `github.ref_protected`), so a ruleset must cover it;
- after fetching `origin/main` and `origin/dev`,
  `git merge-base --is-ancestor <tag commit> origin/<branch>` succeeds for one
  of them.

Every job that touches a secret (`android`, `macos`, `testflight`, `play`),
and `publish`, needs `version` to succeed with `guarded == 'true'`. Those jobs
run in the `release` environment.

On 2026-11-24 this repository had **no rulesets and no environments**, so the
guard refuses every release until the following are applied. They only add
settings. `R=Rylan-Meilutis/Seds-Ground-Station-Frontend`:

```sh
R=Rylan-Meilutis/Seds-Ground-Station-Frontend

# 1. Release tags: only admins create them; nobody moves or deletes them.
cat > /tmp/release-tags.json <<'JSON'
{"name": "Release tags", "target": "tag", "enforcement": "active",
 "conditions": {"ref_name": {"include": ["refs/tags/v*.*.*b*"], "exclude": []}},
 "rules": [{"type": "creation"}, {"type": "update"}, {"type": "deletion"}, {"type": "non_fast_forward"}],
 "bypass_actors": [{"actor_id": 5, "actor_type": "RepositoryRole", "bypass_mode": "always"}]}
JSON
gh api -X POST "repos/$R/rulesets" --input /tmp/release-tags.json

# 2. Release branches: no force-push, no deletion (direct pushes to dev still work).
cat > /tmp/release-branches.json <<'JSON'
{"name": "Release branches", "target": "branch", "enforcement": "active",
 "conditions": {"ref_name": {"include": ["refs/heads/main", "refs/heads/dev"], "exclude": []}},
 "rules": [{"type": "deletion"}, {"type": "non_fast_forward"}]}
JSON
gh api -X POST "repos/$R/rulesets" --input /tmp/release-branches.json

# 3. The release environment: deployable only from release tags, main and dev.
gh api -X PUT "repos/$R/environments/release" \
  -F 'deployment_branch_policy[protected_branches]=false' \
  -F 'deployment_branch_policy[custom_branch_policies]=true'
gh api -X POST "repos/$R/environments/release/deployment-branch-policies" -f name='v*.*.*b*' -f type=tag
gh api -X POST "repos/$R/environments/release/deployment-branch-policies" -f name=main -f type=branch
gh api -X POST "repos/$R/environments/release/deployment-branch-policies" -f name=dev -f type=branch

# Check
gh api "repos/$R/rulesets" --jq '.[] | {id, name, target, enforcement}'
gh api "repos/$R/environments/release/deployment-branch-policies" --jq '.branch_policies[] | {name, type}'
```

Actor 5 is the repository Admin role. Do not add required reviewers to
`release` unless you want to approve each signing job by hand.

Optional and stronger: move the signing secrets from repository secrets to
`release` environment secrets, so that only jobs passing the environment's
policy can read them. Set each one again from the Mac with
`gh secret set <NAME> --env release -R "$R"`, then delete the repository copy
with `gh secret delete <NAME> -R "$R"`. The workflow reads `secrets.<NAME>`
either way.

## Signing material: only in the release workflow, only from secrets

- `platforms.yml` (push/PR CI) never sees a secret. Android CI APKs are signed
  with a fresh throwaway key and labelled not-for-distribution. macOS CI builds
  are unsigned.
- `tests/test_ci_release.py` enforces where secrets can appear:
  - only in `release.yml` and `promote.yml`;
  - only in the `android`, `macos`, `testflight` and `play` jobs (in
    `promote.yml`, only the store API keys, in its `play` and `testflight`
    jobs);
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
  is no fallback to a development key or to an unsigned DMG. The store jobs
  are the exception: with none of their store secrets set they skip with a
  notice, and `publish` does not depend on them. The Play job does wait for
  `android-launch`, so a build that does not start never reaches a track.
- The store jobs upload no workflow artifacts. The `.p8` and the Play
  service-account key are written only for the upload command and deleted
  right after it.
- Each signing job writes the expiry of its certificate (and of the App Store
  profile) to the job summary. It adds a warning annotation when the expiry is
  under 30 days away, but never fails the job.

## 1. Bump the version

1. `Cargo.toml` `[package].version` (semver, each part at most 99) and
   `Dioxus.toml` `[application].build` (0–999). The Android `versionCode` is
   `major*10^7 + minor*10^5 + patch*10^3 + build`, so it must increase from one
   release to the next. For example, 0.3.1 build 36 is 301036.
2. Optional: write release notes in `docs/release-notes/v<version>b<build>.md`.
   Without that file the release uses GitHub's `--generate-notes`.
3. Merge to `main` or `dev`. The release guard refuses tags on any other
   branch. Earlier releases were tagged from `main`.

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
On 2026-11-24 `APPLE_NOTARY_KEY_P8_B64`, `APPLE_NOTARY_KEY_ID` and
`APPLE_NOTARY_ISSUER_ID` did not exist yet, so **releasing with "skip macOS"
is the default** until the App Store Connect key is created. `APPLE_TEAM_ID`
alone does not enable the Apple ID fallback; the error names the API-key
secrets first.

### iOS → TestFlight

| Secret | Value |
|---|---|
| `IOS_DISTRIBUTION_P12_B64` | base64 of the Apple Distribution certificate and private key, exported as `.p12` |
| `IOS_DISTRIBUTION_P12_PASSWORD` | the `.p12` export password |
| `IOS_APPSTORE_PROFILE_B64` | base64 of the **App Store** provisioning profile for `com.UBSEDS.GS26`, made with that certificate |
| `APPLE_NOTARY_KEY_P8_B64`, `APPLE_NOTARY_KEY_ID`, `APPLE_NOTARY_ISSUER_ID` | the same App Store Connect API key as notarization. It also uploads builds (Developer role or higher) |

On the Mac (`R` as above):

```sh
base64 -i ios-distribution.p12 | gh secret set IOS_DISTRIBUTION_P12_B64 -R "$R"
gh secret set IOS_DISTRIBUTION_P12_PASSWORD -R "$R"
base64 -i GS26_AppStore.mobileprovision | gh secret set IOS_APPSTORE_PROFILE_B64 -R "$R"
```

The `testflight` job runs these steps:

1. `release-store-check --store testflight`: with none of the three `IOS_*`
   secrets set, it ends with a "TestFlight upload skipped" notice. With some
   set, any missing one (including the `APPLE_NOTARY_*` key) fails the job.
2. `release-ios-signing`: decodes the profile into `$RUNNER_TEMP` and refuses
   it unless it is an App Store profile whose `application-identifier` is
   `<team>.com.UBSEDS.GS26`: no device list, not enterprise, no
   `get-task-allow`, not expired. It imports the `.p12` into a temporary
   keychain, then requires exactly one Apple Distribution identity whose
   certificate the profile lists.
3. `release-store-build`: `build.py --target ios --release --final`. That is
   the existing distribution path. `package_ios_ipa_with_script` with
   `sign_kind="distribution"` runs `scripts/ios_package_sign.sh`, which
   chooses the identity from the keychain search list by the profile's
   certificates, applies the profile's entitlements and zips
   `dist/ipas/UBSEDS GS.ipa`.
4. `release-store-verify` checks:
   - the bundle id;
   - `CFBundleShortVersionString` is the Cargo version and `CFBundleVersion`
     is the Dioxus build;
   - the embedded App Store profile;
   - `codesign --verify --deep --strict`;
   - the Apple Distribution authority.
5. `release-store-upload`: `xcrun altool --upload-app --type ios --apiKey <id>
   --apiIssuer <issuer>`. `AuthKey_<id>.p8` is placed in
   `$RUNNER_TEMP/seds-release-signing/appstoreconnect-home/.appstoreconnect/private_keys`,
   which is `HOME` / `API_PRIVATE_KEYS_DIR` for that command only, and is
   deleted when altool exits. Rerunning a build number that App Store Connect
   already has ("Redundant Binary Upload") is reported, not failed.

The build shows up in App Store Connect → TestFlight after processing. It is
never submitted for App Store review; that stays a manual step.

**The Apple Watch app is not in the IPA.**

- `build_targets.py` refuses `--final` for the `watchos` target.
- There are no App Store profiles for the watch app or its widget extension.
- The iOS app is signed with `codesign` by `ios_package_sign.sh`, not by
  `xcodebuild`. So Xcode cloud signing (`-allowProvisioningUpdates` with
  `-authenticationKeyPath` / `-authenticationKeyID` /
  `-authenticationKeyIssuerID`), which could create the missing profiles with
  the App Store Connect key, does not apply here.

To ship the watch app later, choose one:

- Create the two App Store profiles in the developer portal and provide them
  as `IOS_WATCH_DISTRIBUTION_MOBILEPROVISION` /
  `IOS_WATCH_WIDGETS_DISTRIBUTION_MOBILEPROVISION`, which `build.py` already
  reads. `--final` then has to allow `watchos`.
- Move the watch build to an `xcodebuild archive` with cloud signing.

### Android → Google Play testing track

| Secret | Value |
|---|---|
| `PLAY_SERVICE_ACCOUNT_JSON_B64` | base64 of a Google Cloud service-account JSON key that has access to the app in Play Console |
| `ANDROID_UPLOAD_KEYSTORE_B64` etc. | the same upload key as the APKs (above) |

Once:

1. Play Console: create the app `com.UBSEDS.GS26` and enrol it in Play App
   Signing with the upload key above. Upload the **first** AAB to internal
   testing by hand; the API cannot create an app. Build that AAB with
   `python build.py --target android --release --final --package-type aab`.
2. Google Cloud: in a project, enable the "Google Play Android Developer API",
   then create a service account and a JSON key for it.
3. Play Console → Users and permissions: invite the service account's email,
   with access to `com.UBSEDS.GS26` only and the "Release to testing tracks"
   permission (plus "View app information"). Do **not** grant "Release to
   production"; the workflow refuses production anyway, and without that
   permission Play refuses it too.
4. Set the secret, then delete the JSON file:
   `base64 -i play-uploader.json | gh secret set PLAY_SERVICE_ACCOUNT_JSON_B64 -R "$R"`.

The `play` job:

1. Builds `build.py --target android --release --final --package-type aab`
   (arm64-v8a). `_finalize_android_release` signs it with jarsigner and the
   upload key.
2. Checks:
   - with `jarsigner -verify` and `keytool -printcert -jarfile`, that the AAB
     is signed only by the upload certificate, and that this certificate
     matches `ANDROID_EXPECTED_CERT_SHA256`;
   - with bundletool, that the package is `com.UBSEDS.GS26` and the
     `versionCode` is the one derived from the tag.
3. Uploads through the Play Developer API directly, with stdlib plus `openssl`
   for the service-account JWT and no third-party action. In one edit it
   uploads the bundle, sets the selected track (`PLAY_TRACK`, default
   `internal`) to exactly that `versionCode` with status `completed`, adds the
   release notes, and commits.

Special cases:

- If Play still treats the app as a draft (never published), the release is
  created as a draft. Roll it out in Play Console.
- If the `versionCode` is already on the selected track (a rerun), nothing is
  uploaded.
- If the `versionCode` was used on another track, the job fails. Promote it
  with `promote.yml`, or bump the Dioxus build.

### Certificate expiry

| Credential | Expires |
|---|---|
| Developer ID Application (`MACOS_DEVELOPER_ID_P12_B64`) | 2027-02-01 |
| Apple Distribution (`IOS_DISTRIBUTION_P12_B64`) | 2027-02-04 |
| App Store profile (`IOS_APPSTORE_PROFILE_B64`) | 2027-02-04 |
| Android upload certificate | printed in the `android` and `play` job summaries |

The jobs warn starting 30 days before expiry. To renew: renew in the developer
portal, export again and update the secret. Regenerate the App Store profile
with the new distribution certificate.

## Testing tracks and groups

By default a release reaches only internal testers: the Play **internal**
track, and the TestFlight internal testers (who get every processed build).
Wider testing is opt-in and never reaches production.

| Setting | Where | Values | Default |
|---|---|---|---|
| `play_track` | Release run input (manual runs) | `internal`, `alpha` (closed testing), `beta` (open testing) | `internal` |
| `PLAY_TRACK` | repository **variable** (tag pushes) | the same three | `internal` when unset |
| `testflight_groups` | Release run input (manual runs) | comma-separated TestFlight beta group names; `none` for internal testers only | empty: use the variable |
| `TESTFLIGHT_GROUPS` | repository **variable** (tag pushes, and manual runs with an empty input) | the same | unset: internal testers only |

- `release-store-check` validates both before any build. `production`, or
  any other track name, fails the job. The Play calls themselves accept only
  the three testing tracks.
- A manual run uses its `play_track` input (default `internal`) and ignores
  the `PLAY_TRACK` variable. A tag push uses the variable.
- The workflows only read these variables. Rylan or the Mac agent sets them,
  for example:

  ```sh
  gh variable set PLAY_TRACK -R "$R" --body alpha
  gh variable set TESTFLIGHT_GROUPS -R "$R" --body "Launch Crew"
  gh variable delete TESTFLIGHT_GROUPS -R "$R"   # back to internal testers only
  ```

- Release notes: if `docs/release-notes/<tag>.md` exists, its text becomes
  the Play release notes (`en-US`, cut to 500 characters) and the TestFlight
  "What to Test" (`en-US`, cut to 4000 characters, set only when groups are
  named). Markdown is passed through as written, so keep the first 500
  characters readable as plain text.

### Play tracks

The `alpha` and `beta` tracks must exist and have testers set up in Play
Console (closed testing: a tester list or Google group; open testing: enabled
countries). An app that has never been published accepts only draft
releases, as on internal.

### TestFlight groups

With `TESTFLIGHT_GROUPS` set, after the altool upload (or a "Redundant Binary
Upload" rerun) `release-store-upload` uses the App Store Connect API with the
same `APPLE_NOTARY_KEY_*` key (an ES256 JWT signed by `openssl`; the `.p8` is
on disk only while it signs):

1. Finds the app by bundle id, then polls `GET /v1/builds` (app, build number,
   marketing version, iOS) every minute until the build is `VALID`. After 60
   minutes it gives up with a **warning**, not a failure; add the groups later
   with `promote.yml`. `FAILED` or `INVALID` processing fails the job.
2. Looks the names up in `GET /v1/betaGroups`. An unknown name fails the job
   and lists the app's groups.
3. If release notes exist, sets "What to Test" (`betaBuildLocalizations`).
4. Adds the build to all the groups
   (`POST /v1/builds/{id}/relationships/betaGroups`).
5. If any group is **external**, submits the build for *beta* app review
   (`POST /v1/betaAppReviewSubmissions`), unless it already was. External
   testers get it after Apple approves.

The driver refuses every App Store Connect call that is not on its allowlist,
so it cannot submit for App Store review or edit an App Store version.

Before the first external group:

- Fill in App Store Connect → TestFlight → Test Information (beta app
  description, feedback email, review contact).
- The app has no `ITSAppUsesNonExemptEncryption` in its Info.plist, so each
  build needs the export-compliance answer in App Store Connect. Until it has
  one, beta review cannot start (the job warns).
- The API key needs the App Manager (or Admin) role to manage beta groups and
  submit for beta review. Developer is enough only for the upload.

The `testflight` job timeout is 180 minutes, to fit the processing wait.

### Promotion (`promote.yml`)

Actions → Promote → Run workflow, **from `main` or `dev`**:

| Input | Effect |
|---|---|
| `tag` | the released tag, e.g. `v0.3.1b36` |
| `play_from_track` / `play_to_track` | put that tag's `versionCode` from `internal` or `alpha` onto a later track (`alpha`, `beta`). `none` (default) leaves Play alone. The source track keeps its release |
| `testflight_groups` | add that tag's TestFlight build to these groups (waits up to 10 minutes for processing). Empty leaves TestFlight alone |

```sh
gh workflow run promote.yml -R "$R" --ref dev -f tag=v0.3.1b36 -f play_from_track=internal -f play_to_track=alpha
gh workflow run promote.yml -R "$R" --ref dev -f tag=v0.3.1b36 -f testflight_groups="Launch Crew"
```

It is guarded like the release: `release-guard` (protected ref, tag on `main`
or `dev`), and its store jobs run in the `release` environment. It builds and
signs nothing, and needs only `PLAY_SERVICE_ACCOUNT_JSON_B64` or the
`APPLE_NOTARY_KEY_*` secrets. Promotion only goes up
(internal → alpha → beta). Play production and App Store review stay manual,
in the consoles.

## 3. Cut the release

```sh
git checkout main && git pull
git tag -a v0.3.1b36 -m "Version 0.3.1 Build 36"
git push origin v0.3.1b36
```

- The tag must be on `main` or `dev`, and only an admin can push it (tag
  ruleset).
- To re-run, or to release a tag without macOS, use Actions → Release → Run
  workflow **from `main` or `dev`**, with `tag` set to e.g. `v0.3.1b36`, "skip
  macOS" as needed and **"dry run" unticked**. Or:
  `gh workflow run release.yml -R "$R" --ref main -f tag=v0.3.1b36 -f skip_macos=true -f dry_run=false`.
  Add `-f play_track=alpha` and/or `-f testflight_groups="Launch Crew"` to
  widen testing for that run (see
  [Testing tracks and groups](#testing-tracks-and-groups)).
- A manual run is a **dry run by default**:
  - it builds and signs: Android, with the apksigner check against
    `ANDROID_EXPECTED_CERT_SHA256`, macOS unless skipped, and the TestFlight
    IPA and Play AAB, which it also verifies (signer, version, iOS SDK and
    deployment target, 16 KB page alignment);
  - it keeps the signed assets as the run's `release-<target>` artifacts for
    7 days;
  - it does not create or edit a GitHub Release, and does not upload to
    TestFlight or Play (the track and group inputs are ignored);
  - it builds the **head of the branch it was started from** (`main` or `dev`),
    so the tag does not have to exist yet. Pushing a tag would publish. The
    `tag` input still names the version and must match `Cargo.toml` and
    `Dioxus.toml`; the guard still requires a protected branch and that the
    commit is on `main` or `dev`.

  Use it to prove signing:
  `gh workflow run release.yml -R "$R" --ref dev -f tag=v0.3.1b36 -f skip_macos=true`.
  A tag push always publishes.
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
