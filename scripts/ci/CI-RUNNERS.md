# Frontend CI runners

Both providers run the SAME actual `build.py --target <target> --debug|--release --no-sign` through `scripts/ci/frontend.py`. Coverage is every target in `scripts/build_targets.py` as a release build, plus debug builds for web, linux, android_sim and wearos_sim (`"debug"` in `platforms.json`): 21 jobs per provider. This means supported architecture/build variants, not every historical OS/Xcode version. iOSdevice builds an unsigned app; simulators are built, never launched. Android builds APK variants signed with a fresh job-local development certificate. Each Android artifact includes `CI-DEVELOPMENT-SIGNED-NOT-FOR-DISTRIBUTION.txt`; the private key is never uploaded. These APKs are not production releases or an identity for updating installed releases. Local user signing settings remain unchanged. This is not distribution signing, notarization, device acceptance or store upload. Missing platform capability is a failed/pending gate, never successful Linux emulation.

GitHub uses real hosted labels Ubuntu24.04 x64/ARM64, Windows2022/MSVC, `xcode-27` (macOS 27 ARM64, Xcode 27.2) and macOS15-intel. Native host+architecture is checked before bootstrap and build.

Apple silicon targets (macos, ios, watchos, ios_sim, watchos_sim) build with `DEVELOPER_DIR=/Applications/Xcode_27.2.app`: App Store Connect requires the iOS 27 SDK (ITMS-90725), and the release workflow's TestFlight and Apple silicon DMG jobs use the same image. GitHub's `xcode-27` image is Apple silicon only and Xcode 27 does not run on Intel Macs, so macos-intel, ios_sim_intel and watchos_sim_intel stay on `macos-15-intel` with Xcode 26 (ios_sim_intel pins Xcode 26.3).

The xcode-27 image runs macOS 27, which prints a deprecation WARNING for every `hdiutil create` and `hdiutil imageinfo`. On macOS 27+ `build.py` builds the DMG with `diskutil image create from --format UDZO --volumeName` (an APFS volume; `diskutil` offers no HFS+ for images) and checks it with `diskutil image info --plist`; `hdiutil verify` prints no warning and stays. Older hosts (macos-15-intel) keep `hdiutil create -fs HFS+`.

Android release builds must support 16 KB memory pages (Google Play, apps targeting Android 15+). The NDK is r28c (28.2.13676358), which links 16 KB-aligned ELF segments by default, and `build.py` also passes `-z max-page-size=16384`. `scripts/check_android_alignment.py` fails the build if any 64-bit `.so` in an APK/AAB has a LOAD segment below 16 KB alignment, or an uncompressed one starts off a 16 KB zip boundary. The hosted image presets `ANDROID_NDK_ROOT` to its own r27 default, so the workflows set `ANDROID_NDK_HOME` and `ANDROID_NDK_ROOT` to the pinned NDK and `build.py` prefers `ANDROID_NDK_HOME`.

The phone app compiles against API 36 (Play's target requirement) with the Android Gradle plugin from the Dioxus 0.7.10 template, AGP 8.7.0, which was tested only up to 35 and otherwise prints a fatal "We recommend using a newer Android Gradle plugin" warning. `build.py` writes `android.suppressUnsupportedCompileSdk=36` into the generated `gradle.properties`; the build itself was verified at API 36 on CI. Remove it once the template's AGP supports 36.

`build.py` also pins the generated app's `ndkVersion` to the same r28c and sets `ndk { debugSymbolLevel = "SYMBOL_TABLE" }` on the release build type. The Dioxus template writes `packaging { jniLibs.keepDebugSymbols.add(...) }` inside `getByName("debug")`, but `packaging` there resolves to `android.packaging`, so it covered every build type: `stripReleaseDebugSymbols` copied `libmain.so` as is, and `extractReleaseNativeSymbolTables` then skipped it as "already stripped" (both only at `--info`). Build 38 shipped an 84 MB `libmain.so` with `.symtab`/`.debug_*` and no symbols for Play. `build.py` removes that block and keeps debug symbols for the debug variant only (`androidComponents { onVariants(selector().withBuildType("debug")) { ... } }`). Android release builds now fail `check_android_alignment.py --release` if a library has `.debug_*` sections or the AAB lacks `BUNDLE-METADATA/com.android.tools.build.debugsymbols/<abi>/<lib>.so.sym`. Debug builds keep their symbols. A local release build needs NDK 28.2.13676358 installed (`sdkmanager "ndk;28.2.13676358"`); AGP downloads it itself only when the SDK licences are accepted.

### Build speed (GitHub)

Compiling the dx CLI took 20-35 minutes of every Rust job. `platforms.yml` restores `.ci-tools/dx` from the Actions cache (key `dx-v1-<os>-<arch>-<target>--<rust>-<dioxus>`); `frontend.py bootstrap` uses it only if `dx --version` reports exactly the dioxus version the fresh lock resolved, otherwise it deletes it and builds dx again, and release rows on `push` save the new copy. Pull requests never save. `release.yml` restores the same keys before each bootstrap but never saves; a tag run reads the caches saved by `main`'s platform CI, and a dry run on dev also reads dev's. The same version check applies, so a release builds dx itself whenever no matching copy is cached. Hosted jobs set `CI_CARGO_JOBS=cores` so Cargo uses every core; GitLab's shared Docker runners keep 2. `max-parallel: 20` leaves the throttling to GitHub's own limits (20 jobs, 5 macOS).

Compiled dependencies (not the app's own crates) are cached with `Swatinem/rust-cache`, pinned by commit, after bootstrap has resolved `Cargo.lock`: key `v1-deps-<target>-<profile>` plus the toolchain, lock and `CARGO_*`/`RUST*` environment hashes, with fallback to the nearest older lock. Only release rows of the targets where it saved 2.5 minutes or more use it (`"deps_cache"` in `platforms.json`: linux, linux-arm64, macos, macos-intel, android, android_sim, ios_sim_intel, watchos_sim_intel); elsewhere restoring 350-700 MB took as long as it saved. Only dev pushes save it, so the dependency caches total about 4 GB and, with dx and Gradle (about 1.1 GB, also saved by main for releases), stay under GitHub's free 10 GB per repository; above that GitHub evicts the least recently used entries, and more than 10 GB needs a payment method on the account. `release.yml` restores `<target>-release` and never saves; a dry run on dev finds dev's entries, a tag run (which reads only main's) does not. Before saving, the action drops the workspace crates, the dx bundle output and anything a week old. It also restores Cargo's registry index as it was when the entry was saved, which can be older than the lock bootstrap has just resolved (hosted run 45950452142: lock `cc` 1.12.0, restored index up to 1.11.3, offline build failed), so `frontend.py refetch` runs `cargo fetch --locked` again right after the restore. GitHub keeps 10 GB of caches per repository and evicts the least recently used, so an evicted entry costs one cold build, never a failure.

### Android launch smoke test (GitHub only)

A build can succeed and the app still not start, so `platforms.yml` job `android-launch` installs the APKs the build job made on x86_64 emulators (ubuntu-24.04 with KVM, `reactivecircus/android-emulator-runner` pinned to a commit) and runs `scripts/android_launch_smoke.py`: `am start -W` of the launcher activity, about 45 s of watching, then logcat, `dumpsys activity activities` and a screenshot (uploaded as `android-launch-*` artifacts). It fails when the process is gone (`pidof`), when logcat from the app's processes shows a `FATAL EXCEPTION`, a `java.lang.*Error` (UnsatisfiedLinkError, NoClassDefFoundError, NoSuchMethodError: missing or stripped `libmain.so`, wrong ABI, JNI classes renamed by R8), a Rust panic, a native crash (`Fatal signal`, the `DEBUG` dump, SIGSEGV/SIGABRT), an ANR or a force-finish, when the launcher activity is not the resumed one, when the app never logs `[startup] app rendered`, or when the middle of the screenshot is one flat colour (a blank web view). Rust's own log lines reach logcat under the `GS26` tag (`src/debug_log.rs`); before, stdout on Android went nowhere and a panic left no trace there.

| Matrix row | Why |
|---|---|
| `android_sim` debug, API 36 | the debug path (unstripped, no R8) |
| `android_sim` release, API 36 | the store path: R8, stripping, symbol extraction, bundletool universal APK |
| `android_sim` release, API 24, `default` image, 2 GB RAM | minSdk: native code that needs newer libc or newer Android APIs than the manifest allows fails here. Release only, to keep the cost down. The image's built-in WebView (52) is years older than what real Android 7 devices get through Play and cannot run the page, so only the launch log line is required (`--require-launch-log --no-screen-check`). With 4 GB the image never finishes booting |
| `wearos_sim` debug, API 34 Wear image | the Wear OS app (Kotlin only, so no render log line; the screenshot still has to show content). Debug, because the release Wear APK is unsigned in this workflow and will not install |

The runtime permissions are granted at install (`adb install -g`), so a permission dialog does not cover the activity; refusing permissions is not covered. The install waits for the package manager to answer (`sys.boot_completed` can come first), uses `--no-streaming` below API 26 (a streamed install hung on the API 24 emulator in CI), and retries once. The script also wakes the display and sets a 30-minute screen timeout for the check, then restores the old value. On main run 45963903338 the API 24 image slept after its one-minute timeout, which paused the app before the check ran. A local API 24 emulator with a 15-second timeout reproduced it. Emulator output stays out of the build step's warning gate; the build steps themselves keep warnings fatal. `release.yml` repeats the API 36 release launch from the tag's commit as `android-launch` (no secrets: it signs with the CI development key) and `publish`, `dry-run` and `play` wait for it. Hosted arm64 runners expose no KVM and macOS runners no nested virtualisation, so the arm64 store APK itself is not launched; the x86_64 build goes through the same Rust, Gradle and packaging code.

### GitLab runners (gitlab.rylanswebsite.com, project 80)

The instance has only generic, untagged instance runners: 9 and 10, plus 4,
which is stale. They use the Docker executor (SEDSnet's jobs run on them with
`image:`). The runner API needs a token, so this was worked out from the
configuration, not read from the server. The binding is in the `gitlab` column
of `platforms.json`.

- **Generic Docker runners, no tags:** `ci-contracts`, plus
  `frontend-build-docker` for web, android, android-armv7, android_sim,
  wearos and wearos_sim. These jobs use `python:3.11-bookworm` and source
  `scripts/ci/gitlab_docker_prereqs.sh`, which installs the build packages,
  Node 22.23.5, rustup, JDK 17 and Android cmdline-tools 13114758 with the
  SDK/NDK packages. Every download is pinned by SHA-256, except that
  rustup-init is checked against its own published checksum. The runner must
  be x86_64; the script and `validate_host` refuse anything else.
- **Tagged native runners, `frontend-build`:** these 12 jobs are created only
  when the project CI variable `GS_UI_NATIVE_RUNNERS` is `true`. By default a
  pipeline has 10 jobs (22 with them). With all 35 of the earlier matrix,
  GitLab refused to create the pipeline
  ("Project exceeded the allowed number of jobs in active pipelines"), and
  without the runners those jobs would only stay pending. Register the
  runners first, then the project owner sets the variable (Settings → CI/CD →
  Variables). Raising the instance's `ci_active_jobs` plan limit may also be
  needed. Every job is `interruptible`, so with "Auto-cancel redundant
  pipelines" on, a new push cancels the previous pipeline.
  - Desktop Linux packages (Flatpak, AppImage/FUSE, Ubuntu 24.04
    makepkg/pacman 6.0.2) do not work in an unprivileged Debian container.
  - Windows and Apple targets need their own OS.

| Tag | Required native host |
|---|---|
| gs-ui-linux-x64 | Linux x86_64 Ubuntu 24.04 (desktop `linux` target only) |
| gs-ui-linux-arm64 | Linux aarch64 |
| gs-ui-windows-x64 | Windows x86_64 with MSVC environment/WindowsSDK/WebView2 tooling |
| gs-ui-macos-arm64 | AppleSilicon macOS 26.6+ with full Xcode 27/device+simulatorSDKs |
| gs-ui-macos-x64 | Intel macOS15+full Xcode/device+simulatorSDKs |

Register them as shell runners on the machine that provides the host, with a
runner authentication token from Admin → CI/CD → Runners → New instance runner
(or project 80 → Settings → CI/CD → Runners), created with the matching tags
and "Run untagged jobs" **off**:

```sh
# Apple silicon Mac (macos, ios, watchos, ios_sim, watchos_sim)
gitlab-runner register --non-interactive --url https://gitlab.rylanswebsite.com \
  --token "$RUNNER_TOKEN" --executor shell --description gs-ui-macos-arm64
# Intel Mac (macos-intel, ios_sim_intel, watchos_sim_intel): same with gs-ui-macos-x64
# Ubuntu 24.04 x86_64 (linux): --executor shell, gs-ui-linux-x64
# Ubuntu 24.04 aarch64 (linux-arm64): --executor shell, gs-ui-linux-arm64
# Windows x64 (windows): --executor shell --shell pwsh, gs-ui-windows-x64
```

With the current runner flow the tags belong to the token (set when creating
the runner in the UI, `tag_list` in the API), not to `register`. On a legacy
registration token, add `--tag-list gs-ui-macos-arm64 --run-untagged=false`
to `register`. Keep these runners off any workstation holding signing
material. GitLab project 80 publishes nothing: releases, signing and store
uploads exist only in GitHub's `release.yml`.

These GitLab registrations have NOT been inspected or executed by this patch. Configure isolated ephemeral shell runners with Python3.11 (`python` on PATH), Rustup/Cargo, Node22/npm and git. No hardware/devices, Apple signing identities, provisioning profiles, upload/signing/notary credentials or production secrets should be installed. Untrusted merge requests must not execute on a privileged persistent crew workstation. Jobs deliberately contain no signing/deployment secrets or token printing. Both normaljob andfailure logs/artifacts are retained7days; inspect before sharing if upstream tools unexpectedly print sensitive host state.

Linux prerequisites: build-essential/pkg-config, GTK3/WebKit2GTK4.1, appindicator/librsvg/libxdo/libssl/libclang dev packages, cmake/ninja/patchelf, file/zip/unzip, FUSE2. Ubuntu24.04 package name is libfuse2t64. GitHub installs them explicitly; provision the equivalent on GitLab. Android jobs require JDK17, platformsandroid35, buildtools35.0.0, platforms;android-36, build-tools;36.0.0, platform-tools and NDK 28.2.13676358 (r28c) with accepted SDK licences; export ANDROID_HOME and ANDROID_NDK_HOME (and ANDROID_NDK_ROOT if the host presets it to another NDK). GitHub sets these through setup-java/setup-android+sdkmanager. macOS requires completed Xcode first-launch/licence; no signing required. Windows runner must initialize the MSVC developer environment; GitHub runs the newest Visual Studio's `vcvars64.bat` (found with vswhere) and exports what it changes, as `ilammy/msvc-dev-cmd` did; that action has no Node 24 release.

Each fresh job installs Rust1.99.0+Clippy, resolves the application Cargo.lock only when absent, and derives exact DioxusCLI and wasm-bindgenCLI versions from its resolved dioxus/wasm-bindgen packages. The resolved versions and lock SHA256 are retained in ci-results/resolved-tools.json. DioxusCLI installation uses cargo +1.99.0 install with =version while resolving its own dependencies afresh, matching the application bootstrap policy for published CLI locks containing yanked dependencies. wasm-bindgenCLI uses the official exact-version native release archive for each of the five runner hosts, with hardcoded SHA256 pins, a 64MiB download/256MiB unpacked bound, and strict archive-member validation. Only the expected executable is staged; links and traversal are rejected. Its actual --version output is warning-checked and must match the resolved ABI before publication into .ci-tools. The URL, archive/binary hashes and reported version are retained in ci-results/wasm-bindgen-release.json. A newer resolved ABI without reviewed archive pins fails explicitly; update the pins before rebuilding. This avoids compiling the CLI's unrelated legacy multipart dependencies; no diagnostic is suppressed. Jobs also install esbuild0.25.12; Binaryen133 native archives have hardcoded upstream SHA256 pins. No feature/queue/precision changes or platform-flag overrides. Archive/tool download availability, runner packages and third-party action versions still require real CI qualification; this is not a reproducible-image claim. Never put a stale `.ci-tools` or `artifacts/<target>` into a new job. No Cargo/target caches are configured by this first patch. Each job uses2 build jobs, incremental off; GitHub max4matrix jobs at once,120min timeout. GitLab concurrency is configured on the runners; eachjob2h.

RUSTFLAGS/RUSTDOCFLAGS deny warnings. Inherited CARGO_ENCODED_RUSTFLAGS, CARGO_ENCODED_RUSTDOCFLAGS and target-specific CARGO_TARGET_*_RUSTFLAGS are refused, including empty values, so they cannot silently override the warning policy. The shared subprocess driver also preserves unfiltered tool output and fails nonzero exits OR recognized warning diagnostics; no upstream-warning whitelist or blanket lint suppression. Exact failed logs and build artifacts are uploaded even on failure. An empty platform stagingdirectory is a failure, not a successful build. Actual project/upstream warnings discovered by real CI are blockers to resolve, not pre-waived. The application build.py host-tool/iOS flag separation fix is already part of the reviewed base; this CI patch does not modify it.

Fast contracts validate both matrices against actual target registry, correct native routing, all24 commands/profile flags, no skip/final/production-sign/deploy commands, fatal warnings/nonzero behavior using real Python subprocesses, and actualartifact retention configuration. They do not compile/launch frontend apps or prove runner availability. Local execution does not establish any Apple/Windows/Android build pass.

References: https://docs.github.com/en/actions/reference/runners/github-hosted-runners ; https://docs.gitlab.com/ci/jobs/job_control/#parallelize-large-jobs ; https://github.com/WebAssembly/binaryen/releases/expanded_assets/version_133 .


## Hosted b237 CI corrections

The pipeline keeps Rust and rustdoc `-D warnings` and preserves full tool output.
Dioxus CLI 0.7.10 emits timer-only `warning: Waiting for cargo-metadata...` and
`warning: (Try N) Taking a while...` messages even when metadata succeeds. Only
those exact upstream lines, only during the application build, are separately
recorded as `external_metadata_progress_warnings`; they remain visible in the
console and log. They are not a zero-emitted-warnings claim. Compiler warnings,
SDK/resource/tool warnings and the upstream network/build-lock diagnostic still
fail. Application packages are fetched with the resolved lock before the build;
application Cargo work during the build is offline.

Every prepared tool is resolved to an absolute executable using the child PATH
before spawning, including `wasm-opt.exe` on Windows. Wasm-bindgen, Binaryen and
linuxdeploy downloads retry transient transport/408/429/5xx failures at most
three times (1s/2s backoff); each attempt retains its 120s bound. Downloads
retain the 64MiB default, except Binaryen133's compressed Linux/Windows archives
(105,577,739 to 111,200,018 bytes) use a bounded 128MiB cap. Checksum,
archive and version failures remain fatal and are never retry success. HTTP
error responses are closed before retry. Official linuxdeploy release 430855031
(November 1, 2026) bytes are pinned separately for x86_64/aarch64; a moved
continuous tag fails verification instead of silently changing the tool.

Native icon generation requires Pillow12.0.0. Linux workers need `rpm makepkg libarchive-tools pacman-package-manager
fakeroot flatpak desktop-file-utils appstream gnupg zstd` in addition to the existing GUI/build
packages; GitHub installs these and GitLab tagged runners must provision them.
The driver fails early on missing Linux packagers. The explicitly prepared
native linuxdeploy wins over a stale home cache; ARM fallback selects ARM bytes. Android setup installs
`platform-tools`, not the removed legacy SDK `tools` package, before the existing
explicit SDK 35/36 and NDK installation. Neither provider changes application features,
precision, queues, signing or deployment authority.

References: pinned upstream Dioxus timer implementation:
https://github.com/DioxusLabs/dioxus/blob/v0.7.10/packages/cli/src/workspace.rs ;
Ubuntu24.04 makepkg package: https://packages.ubuntu.com/noble/makepkg ;
official linuxdeploy release: https://github.com/linuxdeploy/linuxdeploy/releases/tag/continuous .


Pinned Dioxus0.7.10 copies custom `bundle.resources` entries as literal files,
not glob patterns. The two old literal globs copied no files; they are removed.
Actual scripts/maps are Rust `include_str!`, replay models/renderers use native
`include_bytes!`, theme presets compile into OUT_DIR, and iOS Assets.car keeps
its existing explicit bundle-copy path. The common icon list retains PNG/ICO;
macOS still generates ICNS from PNG and the existing repo ICNS patch is retained.
No runtime asset or branding source is removed. Legacy artifact renaming now
reports an informational no-op only after no legacy filesystem input exists;
actual rename errors propagate. The mobile debug-only unused vendor helper now
has exactly the desktop/debug cfg predicate of its only caller, with no lint
suppression or function-body change.


## Ubuntu packaging readiness

Ubuntu24's makepkg6 needs the actual `pacman` executable even with `--nodeps`:
its startup resolves that executable before processing PKGBUILD. Provision
`pacman-package-manager` with makepkg; the driver checks `pacman` explicitly.
Missing it was reproduced as SIGUSR1/exit-10 using the unchanged generated
PKGBUILD, then resolved by installing that one package.

AppImage packaging uses its already-owned hicolor icon instead of deploying the
same staged file again as an external `--icon-file`. Only AppDir omits the
redundant size-less pixmaps copy; other Linux packages retain it. Normal strip
processing applies to the staged copy, leaving the original build executable
untouched. Explicit user `NO_STRIP` remains honored, with its real warning still
fatal under CI; the builder no longer forces it.

Generated AppStream metadata derives its identifier, description, project URL
and project license from existing Dioxus/Cargo/LICENSE inputs. AppStream's
lowercase identifier projection and matching AppImage desktop/metainfo filenames
do not change the runtime/Dioxus identifier, desktop contents, binary or icon.
Other Linux package desktop filenames retain their existing names. The newly
generated metadata is CC0-1.0; the application license stays the existing GPL2.
Unknown license-file formats require an explicit SPDX package.license instead
of inventing a claim. No release dates, release history or screenshots are
fabricated. Provision appstreamcli (`appstream`) and GnuPG (`gnupg`). GitHub
`::warning::` annotations are recognized as warnings, including packaging tools'
AppStream diagnostics; no missing-metadata or packaging warning is filtered.

### Version-bound Arch payload packaging

The manual Arch recipe only copies already-built frontend files. It deliberately
contains no source, prepare, build or check stages. Ubuntu24's qualified
makepkg/pacman6.0.2 toolchain runs its internal `-F` fakeroot package phase with
an empty owned `pkg/` directory. Other makepkg versions fail explicitly pending
qualification; this is not a general PKGBUILD/source-build entry point. No
`--nodeps` or signature-skip flags are used. Runtime dependencies remain in the
package metadata and are resolved by the target's package manager at install.

An empty owned temporary pacman database is initialized by makepkg's official
`pacman -Qi` metadata query, without legacy database migration, invented
installed-package records or changes to the host database. Both makepkg's
metadata query and `pacman -Qip` use it. Before copying a gzip Arch package to
`dist`, Python verifies every payload path, type, content hash, mode and root
ownership, plus package identity, architecture and dependency metadata. The
package's own metadata files are permitted; unexpected/missing payload is fatal.
The AppImage/Flatpak package controls use dummy payloads and never establish
application-runtime success. Actual hosted native builds remain the final gate.


## Hosted Linux RPM and Android packaging

RPM's explicit file manifest includes the staged AppStream metainfo file, using
its existing lowercase Dioxus identifier. Unpackaged-file checks remain enabled.
The Android vendor trampoline drops three imports unused by its current body;
compiler warnings remain fatal.

Android CI already provisions Java 17. Bootstrap downloads Google's official
bundletool 1.18.3 release (the latest non-prerelease verified on 2026-11-06),
checks the exact asset size and SHA-256, and checks its reported version. The
build stage checks the same pinned JAR again. CI exports `BUNDLETOOL_JAR` to
`build.py`, whose repo-managed AAB-to-universal-APK stage consumes it. This is
not an assumption that Dioxus reads that variable: Dioxus 0.7.10 selects AAB and
uses Gradle. No mutable latest URL, checksum bypass, warning suppression, or
fallback-success claim is introduced.


`makepkg` invokes `bsdtar` to generate `.MTREE` metadata and write its archive.
On Ubuntu 24.04, provision `libarchive-tools`; the shared bootstrap also checks
for the actual `bsdtar` executable before packaging. Installing the libarchive
runtime library alone does not supply that command. Arch packaging remains
required, and its errors and archive validation remain fatal.

Official Ubuntu package and command reference:
https://packages.ubuntu.com/noble/libarchive-tools ;
https://manpages.ubuntu.com/manpages/noble/en/man1/bsdtar.1.html .


## Android CI development signing and prepared host tools

The CI driver creates a fresh PKCS12 development key under its private `.ci-tools/android-development-signing` directory using the installed Java `keytool`. Passwords are random per job and passed by private mode-0600 file reference, never command-line values or recorded receipts. The build-only key is used explicitly by bundletool for universal APK creation; no deployment credentials, local upload key or user keychain are read. Both debug and release-profile CI APKs carry this development certificate. The signing-mode receipt contains no private key material and the key directory is outside uploaded artifact paths. Missing tools, invalid keys, incomplete signing configuration and every other tool warning still fail the job. This fixes actual unsigned-APK output rather than filtering its warning.

Already-canonical Android artifacts count as recognized input to the idempotent renamer. A directory without recognized Android artifacts still reports the original warning. Container `wasm-opt --version` verification executes directly with the prepared build environment, avoiding a login shell that could replace the pinned Binaryen PATH.

Android packaging fallback requires an explicit Gradle packaging/configuration failure from the current `dx` command and its generated project. A staged directory or historical log is insufficient. Rust compilation failures, including errors preceding the retained bounded diagnostic tail, fail immediately; empty or unknown Android failures cannot package stale output. Known Gradle cache and wasm-bindgen repair retries remain available and must succeed. Exception diagnostics retain at most 128 lines of 4096 characters and redact credential environment values; original streamed compiler output and the CI warning gate remain unchanged.

Non-Android `dx` commands also capture their own bounded failure output through `run(capture_failure=True)`. This preserves current wasm-bindgen and staged Linux packaging repairs without reading historical global logs. Other ordinary `run()` calls retain their existing execution behavior.

A dependency download answered with 429 or 5xx never enters that fallback; it fails the build as a download failure (see below).

## Gradle dependency downloads

Every CI Android and Wear OS build first installs `scripts/ci/maven-central-mirror.gradle` into `.gradle-user-home/init.d`. The init script points every Maven Central repository (plugin, settings, buildscript and project) at Google's mirror, `maven-central.storage-download.googleapis.com/maven2`. Maven Central refused one hosted runner with 429 on every attempt over about 30 minutes (main run 45963903338, android-armv7). With an empty Gradle home, the generated phone project and the Wear project made no requests to Maven Central and resolved everything from the mirror. A plain local `build.py` run does not install it.

Android and Wear OS builds are repeated, at most four times (after 60, 240, 600 and 900 s), only when every root cause in every Gradle failure of the attempt is a download answered with 429 or 5xx, or a timed-out download (`transient_dependency_failures` in `frontend.py`). A warning, a compiler error, a missing artifact or any other cause is final at once, as before. Each attempt keeps its own log and receipt (`<label>-dependency-retry-N`); `<label>-dependency-retries.json` and the job summary record what failed and when it was retried.

Build-only CI (`platforms.yml`) also caches Gradle's downloaded modules and wrapper distribution, as an allow-list of two directories under `.gradle-user-home`. Nothing a build generates is cached: no Gradle project or build output, `.ci-tools`, runner temp directory or keystore. Pull requests restore it but never save it. `release.yml` restores it in its Android jobs and never saves it. `tests/test_ci_gradle_downloads.py` enforces all of this.

## Android release R8 rule notices

Minified Android release builds print one R8 notice per keep rule without a member pattern. On the hosted runners all ten come from AGP's default rules and AndroidX consumer rules, and Gradle still exits 0 with a usable APK. The driver reports exactly that notice from exactly those generated paths under `external_android_r8_rule_notices` in the step receipt, for Android release builds only. Any other warning, including the same text from a rule file in this repository, remains fatal. Revisit this when AGP moves to the R8 major version that drops the implicit constructor keep.


## Releases (production signing)

`platforms.yml` and GitLab stay build-only: no secrets, `--no-sign`, and
throwaway Android development keys. The only workflow that uses production
signing material is the tag-triggered `.github/workflows/release.yml`.
It runs `frontend.py release-*` on the same bootstrap and `build.py` paths,
with `--final` for Android and macOS:

- The Android keystore, Developer ID `.p12`, notary `.p8` and a temporary
  keychain come from repository secrets.
- They live only under `$RUNNER_TEMP/seds-release-signing`.
- They are deleted by an `always()` cleanup step.
- They are never uploaded.

`tests/test_ci_release.py` enforces this. See `docs/releasing.md`.
