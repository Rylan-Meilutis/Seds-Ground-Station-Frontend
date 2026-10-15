# Legacy block FFI maintenance

Source: crates.io block 0.1.6 by Steven Sheldon, MIT license.
The release artifact is retained here because Cocoa still depends on its public
API. Replace the uninhabited private opaque Class enum with an inhabited opaque
repr(C) struct and make the already-default C ABI explicit. Blocks still use the
same raw class pointer and system copy/release functions. No warning suppression.

This removes Rust's future-incompatible uninhabited-static declaration. Validate
macOS linking and the frontend tests whenever this patch changes.
