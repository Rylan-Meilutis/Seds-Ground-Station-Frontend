# Manganis Android pointer-width compatibility

This directory vendors the published manganis 0.7.10 package, licensed MIT OR Apache-2.0. Original package source commit: 57d6794ad60b949e5bd8aa282f6f8c3dc97a365e. Cargo.toml is the published normalized manifest; registry metadata and the crate-local lock are not required by this path dependency.

The sole Rust change is the exact accepted upstream callback hunk from DioxusLabs/dioxus commit d5c90910de5de5d4af76cf1e2f3efec7e76fcf77, PR https://github.com/DioxusLabs/dioxus/pull/5641. It removes the 64-bit-only compilation gate and explains the existing two-jlong pointer contract. JNI signatures, pointer reconstruction/call, features, dependencies and all other Rust bodies remain unchanged.

This backport does not independently certify arbitrary JNI inputs or Android runtime behavior. Validate both armv7-linux-androideabi and aarch64-linux-android; cross-check metadata compilation alone does not establish device execution.
