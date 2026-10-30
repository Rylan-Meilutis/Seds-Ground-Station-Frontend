# Physical iPhone startup evidence

A successful simulator launch does not prove that the installed device build can
start. Device and simulator executables can both be arm64. Packaging now checks
the requested CPU and each image's `LC_BUILD_VERSION` platform before copying or
changing its plist, including embedded native libraries. It rejects ambiguous,
opposite-profile and unproven dist-only artifacts. These checks prevent a cached
simulator/device mix-up; they do not establish the cause of a particular crash.

## Collect evidence on the phone without SSH

1. Reproduce the failure once and note the time and iOS version.
2. Open Settings, then Privacy & Security → Analytics & Improvements → Analytics
   Data. Find the recent report matching `groundstation_frontend`, `UBSEDS GS`,
   `SEDS GS`, or bundle ID `com.UBSEDS.GS26`. Share the complete `.ips` report to
   Files and attach it when reporting the issue. If the only matching entry is a
   `JetsamEvent`, include that instead. [Apple's crash-report instructions](https://developer.apple.com/documentation/xcode/acquiring-crash-reports-and-diagnostic-logs)
3. With the diagnostic build from frontend dev `5351511` or later installed, open
   Files → Browse → On My iPhone → SEDS GS (or UBSEDS GS) → GS26Diagnostics. Copy
   both `launch-current.txt` and `launch-previous.txt` before another series of
   launches. They contain fixed Rust startup milestones and panic line/column,
   not credentials. See [startup diagnostics](ios-startup-diagnostics.md).

If the diagnostics folder is absent, first verify which build is installed. An
older build or failure before entering Rust may have no folder; absence alone
does not identify the crash cause. Native signals, Objective-C exceptions,
loader/signature rejection and OS termination can bypass the Rust panic hook.

When a local Mac is available, a USB connection is enough: Finder → iPhone →
Files → SEDS GS, then drag GS26Diagnostics to the Mac. SSH is unnecessary. Copy
the reports before reinstalling the app. [Apple's File Sharing instructions](https://support.apple.com/en-us/119585)

## Match the installed artifact on the Mac

Keep the exact installed IPA/app and matching debug symbols. Record its build
command, frontend commit, device model and iOS version. These read-only commands
inspect the app that was installed; they do not re-sign or rebuild it:

```sh
ios_app="dist/UBSEDS GS.app"
ios_executable=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$ios_app/Info.plist")
ios_binary="$ios_app/$ios_executable"
file "$ios_binary"
xcrun dwarfdump --uuid "$ios_binary"
otool -arch all -l "$ios_binary"
otool -arch all -L "$ios_binary"
codesign --verify --deep --strict --verbose=2 "$ios_app"
```

The load commands show CPU/platform, runtime search paths and linked libraries.
Check embedded frameworks/dylibs too. Preserve their signatures, architectures
and actual files; a plist label cannot convert a simulator Mach-O into a device
Mach-O. A matching binary UUID is needed to relate symbols to the crash report.
[Apple's symbol-information guidance](https://developer.apple.com/documentation/xcode/building-your-app-to-include-debugging-information)

Use the `.ips` termination/exception details and the last recorded Rust milestone
to choose the next investigation. Do not change entitlements, trust policy,
framework embedding or signing identities based only on an instant exit.
