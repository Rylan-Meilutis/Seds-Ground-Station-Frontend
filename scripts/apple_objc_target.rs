//! Exact SDK/architecture selection for the native shim; no SDK invocation here.
pub fn target_spec(target: &str) -> Option<(&'static str, &'static str, &'static str)> {
    match target {
        "aarch64-apple-ios" => Some(("iphoneos", "arm64-apple-ios13.0", "ios-arm64")),
        "aarch64-apple-ios-sim" => Some((
            "iphonesimulator",
            "arm64-apple-ios13.0-simulator",
            "ios-sim-arm64",
        )),
        "x86_64-apple-ios" => Some((
            "iphonesimulator",
            "x86_64-apple-ios13.0-simulator",
            "ios-sim-x86_64",
        )),
        "x86_64-apple-darwin" => Some(("macosx", "x86_64-apple-macosx10.15", "macos")),
        "aarch64-apple-darwin" => Some(("macosx", "arm64-apple-macosx10.15", "macos")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn device_and_both_simulator_architectures_have_distinct_correct_sdk_and_cache() {
        assert_eq!(
            target_spec("aarch64-apple-ios"),
            Some(("iphoneos", "arm64-apple-ios13.0", "ios-arm64"))
        );
        assert_eq!(
            target_spec("aarch64-apple-ios-sim"),
            Some((
                "iphonesimulator",
                "arm64-apple-ios13.0-simulator",
                "ios-sim-arm64"
            ))
        );
        assert_eq!(
            target_spec("x86_64-apple-ios"),
            Some((
                "iphonesimulator",
                "x86_64-apple-ios13.0-simulator",
                "ios-sim-x86_64"
            ))
        );
    }
    #[test]
    fn desktop_apple_sdk_selection_is_preserved() {
        assert_eq!(
            target_spec("x86_64-apple-darwin"),
            Some(("macosx", "x86_64-apple-macosx10.15", "macos"))
        );
        assert_eq!(
            target_spec("aarch64-apple-darwin"),
            Some(("macosx", "arm64-apple-macosx10.15", "macos"))
        );
        assert_eq!(target_spec("aarch64-unknown-linux-gnu"), None);
    }
}
