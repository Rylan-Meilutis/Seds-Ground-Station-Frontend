# syntax=docker/dockerfile:1
FROM rust:1.99-bookworm AS base
ARG DIOXUS_VERSION=0.7.10
ENV DEBIAN_FRONTEND=noninteractive CARGO_HOME=/opt/cargo RUSTUP_HOME=/usr/local/rustup CARGO_BUILD_JOBS=4
ENV PATH=/opt/cargo/bin:/usr/local/cargo/bin:$PATH
RUN apt-get update && apt-get install -y --no-install-recommends python3 pkg-config libgtk-3-dev libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev libssl-dev patchelf file curl unzip zip ca-certificates binaryen fuse libfuse2 && rm -rf /var/lib/apt/lists/*
RUN mkdir -p /opt/cargo && cargo install dioxus-cli --version ${DIOXUS_VERSION} --locked --jobs 4 && rustup target add wasm32-unknown-unknown && chmod -R a+rwX /opt/cargo /usr/local/rustup
WORKDIR /work

FROM base AS android
# Android SDK command-line tools run on Linux x86_64. Target APKs are cross-compiled.
ARG ANDROID_TOOLS=13114758
ENV ANDROID_HOME=/opt/android ANDROID_SDK_ROOT=/opt/android
ENV PATH=/opt/android/cmdline-tools/latest/bin:/opt/android/platform-tools:$PATH
RUN test "$(uname -m)" = x86_64 && apt-get update && apt-get install -y --no-install-recommends openjdk-17-jdk-headless && rm -rf /var/lib/apt/lists/*
RUN mkdir -p /opt/android/cmdline-tools && curl -fsSL "https://dl.google.com/android/repository/commandlinetools-linux-${ANDROID_TOOLS}_latest.zip" -o /tmp/android.zip && unzip -q /tmp/android.zip -d /opt/android/cmdline-tools && mv /opt/android/cmdline-tools/cmdline-tools /opt/android/cmdline-tools/latest && rm /tmp/android.zip
RUN yes | sdkmanager --licenses >/dev/null && sdkmanager "platform-tools" "platforms;android-35" "build-tools;35.0.0" "ndk;27.2.12479018"
RUN rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android && chmod -R a+rwX /opt/android /usr/local/rustup
ENV ANDROID_NDK_HOME=/opt/android/ndk/27.2.12479018
