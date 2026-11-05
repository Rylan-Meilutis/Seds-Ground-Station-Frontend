# Frontend CI runners

Both providers run the SAME actual `build.py --target <target> --debug|--release --no-sign` through `scripts/ci/frontend.py`. Coverage is every12 target in `scripts/build_targets.py`,24jobs/provider. This means supported architecture/build variants, not every historical OS/Xcode version. iOSdevice builds an unsigned app; simulators are built, never launched. Android builds APK variants. This is not distribution signing, notarization, device acceptance or store upload. Missing platform capability is a failed/pending gate, never successful Linux emulation.

GitHub uses real hosted labels Ubuntu24.04 x64/ARM64, Windows2022/MSVC, macOS15 ARM64 and macOS15-intel. Native host+architecture is checked before bootstrap and build. GitLab needs these provisioned tags (unavailable runners remain pending):

| Tag | Required native host |
|---|---|
| gs-ui-linux-x64 | Linux x86_64, plus AndroidSDK for Android jobs |
| gs-ui-linux-arm64 | Linux aarch64 |
| gs-ui-windows-x64 | Windows x86_64 with MSVC environment/WindowsSDK/WebView2 tooling |
| gs-ui-macos-arm64 | AppleSilicon macOS15+full Xcode/device+simulatorSDKs |
| gs-ui-macos-x64 | Intel macOS15+full Xcode/device+simulatorSDKs |

These GitLab registrations have NOT been inspected or executed by this patch. Configure isolated ephemeral shell runners with Python3.11 (`python` on PATH), Rustup/Cargo, Node22/npm and git. No hardware/devices, Apple signing identities, provisioning profiles, upload/signing/notary credentials or production secrets should be installed. Untrusted merge requests must not execute on a privileged persistent crew workstation. Jobs deliberately contain no signing/deployment secrets or token printing. Both normaljob andfailure logs/artifacts are retained7days; inspect before sharing if upstream tools unexpectedly print sensitive host state.

Linux prerequisites: build-essential/pkg-config, GTK3/WebKit2GTK4.1, appindicator/librsvg/libxdo/libssl/libclang dev packages, cmake/ninja/patchelf, file/zip/unzip, FUSE2. Ubuntu24.04 package name is libfuse2t64. GitHub installs them explicitly; provision the equivalent on GitLab. Android jobs require JDK17, platformsandroid35, buildtools35.0.0, platform-tools and NDK27.2.12479018 with accepted SDK licences; export ANDROID_HOME and ANDROID_NDK_HOME. GitHub sets these through setup-java/setup-android+sdkmanager. macOS requires completed Xcode first-launch/licence; no signing required. Windows runner must initialize the MSVC developer environment; GitHub does so through msvc-dev-cmd.

Each fresh job installs Rust1.99.0+Clippy, resolves the application Cargo.lock only when absent, and derives exact DioxusCLI and wasm-bindgenCLI versions from its resolved dioxus/wasm-bindgen packages. The resolved versions and lock SHA256 are retained in ci-results/resolved-tools.json. DioxusCLI installation uses cargo +1.99.0 install with =version while resolving its own dependencies afresh, matching the application bootstrap policy for published CLI locks containing yanked dependencies. wasm-bindgenCLI uses the official exact-version native release archive for each of the five runner hosts, with hardcoded SHA256 pins, a 64MiB download/256MiB unpacked bound, and strict archive-member validation. Only the expected executable is staged; links and traversal are rejected. Its actual --version output is warning-checked and must match the resolved ABI before publication into .ci-tools. The URL, archive/binary hashes and reported version are retained in ci-results/wasm-bindgen-release.json. A newer resolved ABI without reviewed archive pins fails explicitly; update the pins before rebuilding. This avoids compiling the CLI's unrelated legacy multipart dependencies; no diagnostic is suppressed. Jobs also install esbuild0.25.12; Binaryen133 native archives have hardcoded upstream SHA256 pins. No feature/queue/precision changes or platform-flag overrides. Archive/tool download availability, runner packages and third-party action versions still require real CI qualification; this is not a reproducible-image claim. Never put a stale `.ci-tools` or `artifacts/<target>` into a new job. No Cargo/target caches are configured by this first patch. Each job uses2 build jobs, incremental off; GitHub max4matrix jobs at once,120min timeout. GitLab concurrency is configured on the runners; eachjob2h.

RUSTFLAGS/RUSTDOCFLAGS deny warnings. Inherited CARGO_ENCODED_RUSTFLAGS, CARGO_ENCODED_RUSTDOCFLAGS and target-specific CARGO_TARGET_*_RUSTFLAGS are refused, including empty values, so they cannot silently override the warning policy. The shared subprocess driver also preserves unfiltered tool output and fails nonzero exits OR recognized warning diagnostics; no upstream-warning whitelist or blanket lint suppression. Exact failed logs and build artifacts are uploaded even on failure. An empty platform stagingdirectory is a failure, not a successful build. Actual project/upstream warnings discovered by real CI are blockers to resolve, not pre-waived. The application build.py host-tool/iOS flag separation fix is already part of the reviewed base; this CI patch does not modify it.

Fast contracts validate both matrices against actual target registry, correct native routing, all24 commands/profile flags, no skip/final/sign/deploy commands, fatal warnings/nonzero behavior using real Python subprocesses, and actualartifact retention configuration. They do not compile/launch frontend apps or prove runner availability. Local execution does not establish any Apple/Windows/Android build pass.

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

Native icon generation requires Pillow12.0.0. Linux workers need `rpm makepkg pacman-package-manager
fakeroot flatpak desktop-file-utils appstream gnupg zstd` in addition to the existing GUI/build
packages; GitHub installs these and GitLab tagged runners must provision them.
The driver fails early on missing Linux packagers. The explicitly prepared
native linuxdeploy wins over a stale home cache; ARM fallback selects ARM bytes. Android setup installs
`platform-tools`, not the removed legacy SDK `tools` package, before the existing
explicit SDK35/NDK installation. Neither provider changes application features,
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
