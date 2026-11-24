# Source (do not execute) from a GitLab job running in the python:3.11-bookworm
# image on a generic Docker runner:  . scripts/ci/gitlab_docker_prereqs.sh
#
# Provisions what scripts/ci/frontend.py bootstrap expects to find on a native
# runner, for the targets that build fine in an unprivileged Linux container:
# web, android, android-armv7, android_sim, wearos, wearos_sim. Desktop Linux
# packages (Flatpak, AppImage/FUSE, Ubuntu 24.04 makepkg) still need the
# gs-ui-linux-* shell runners; see scripts/ci/CI-RUNNERS.md.
#
# Every download is pinned by version and SHA-256. Nothing here reads a secret.
# GitLab's job shell already runs with errexit and pipefail.

: "${CI_FRONTEND_TARGET:?CI_FRONTEND_TARGET is required}"
if [ "$(uname -m)" != "x86_64" ]; then
  echo "gitlab_docker_prereqs: needs an x86_64 Docker runner, got $(uname -m)" >&2
  exit 1
fi

GS_NODE_VERSION=22.23.5
GS_NODE_SHA256=cdeb8670a0c81f51b4dc4ac61919f9b1ad917b304bf8221eb939570663d240f8
GS_ANDROID_TOOLS=13114758
GS_ANDROID_TOOLS_SHA256=7ec965280a073311c339e571cd5de778b9975026cfcbe79f2b1cdcb1e15317ee
GS_NDK=27.2.12479018

export DEBIAN_FRONTEND=noninteractive
apt-get update
apt-get install -y --no-install-recommends ca-certificates curl xz-utils unzip zip file pkg-config \
  build-essential libssl-dev libclang-dev clang cmake ninja-build git
case "$CI_FRONTEND_TARGET" in
  android*|wearos*) apt-get install -y --no-install-recommends openjdk-17-jdk-headless ;;
  web) ;;
  *)
    echo "gitlab_docker_prereqs: $CI_FRONTEND_TARGET is not a Docker target" >&2
    exit 1
    ;;
esac

gs_fetch() {  # url sha256 destination
  curl --fail --location --retry 3 --retry-all-errors --silent --show-error "$1" -o "$3"
  echo "$2  $3" | sha256sum --check --quiet -
}

if [ "${CI_FRONTEND_TARGET#wearos}" = "$CI_FRONTEND_TARGET" ]; then
  # Node 22 (esbuild install) and rustup; Wear OS is Kotlin/Gradle only.
  gs_fetch "https://nodejs.org/dist/v${GS_NODE_VERSION}/node-v${GS_NODE_VERSION}-linux-x64.tar.xz" \
    "$GS_NODE_SHA256" /tmp/node.tar.xz
  tar -xJf /tmp/node.tar.xz -C /usr/local --strip-components=1
  rm /tmp/node.tar.xz
  node --version
  # rustup-init's published checksum; frontend.py bootstrap installs the pinned toolchain.
  curl --fail --location --retry 3 --silent --show-error \
    https://static.rust-lang.org/rustup/dist/x86_64-unknown-linux-gnu/rustup-init -o /tmp/rustup-init
  echo "$(curl --fail --location --retry 3 --silent --show-error \
    https://static.rust-lang.org/rustup/dist/x86_64-unknown-linux-gnu/rustup-init.sha256 | cut -d' ' -f1)  /tmp/rustup-init" \
    | sha256sum --check --quiet -
  chmod +x /tmp/rustup-init
  /tmp/rustup-init -y --no-modify-path --profile minimal --default-toolchain none
  rm /tmp/rustup-init
  export PATH="$HOME/.cargo/bin:$PATH"
fi

case "$CI_FRONTEND_TARGET" in
  android*|wearos*)
    export ANDROID_HOME=/opt/android ANDROID_SDK_ROOT=/opt/android
    mkdir -p "$ANDROID_HOME/cmdline-tools"
    gs_fetch "https://dl.google.com/android/repository/commandlinetools-linux-${GS_ANDROID_TOOLS}_latest.zip" \
      "$GS_ANDROID_TOOLS_SHA256" /tmp/android-tools.zip
    unzip -q /tmp/android-tools.zip -d "$ANDROID_HOME/cmdline-tools"
    mv "$ANDROID_HOME/cmdline-tools/cmdline-tools" "$ANDROID_HOME/cmdline-tools/latest"
    rm /tmp/android-tools.zip
    export PATH="$ANDROID_HOME/cmdline-tools/latest/bin:$ANDROID_HOME/platform-tools:$PATH"
    # pipefail: yes ends with SIGPIPE once sdkmanager stops reading.
    (yes || true) | sdkmanager --licenses >/dev/null
    sdkmanager "platform-tools" "platforms;android-35" "build-tools;35.0.0" "platforms;android-36" \
      "build-tools;36.0.0" "ndk;${GS_NDK}"
    export ANDROID_NDK_HOME="$ANDROID_HOME/ndk/${GS_NDK}"
    ;;
esac
