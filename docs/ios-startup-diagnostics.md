# iOS startup diagnostics

The iOS app keeps `Documents/GS26Diagnostics/launch-current.txt` and one previous
launch report. Retrieve the folder through the app's Documents in Files or Finder
File Sharing. These reports contain fixed startup milestones and Rust panic line
and column numbers only; they contain no panic payload, account, token, URL, or
settings. Each report is limited to 64 KiB. Repeated component renders do not grow
the report. Failure to write a report does not prevent launch.

Milestones distinguish entry into Rust, panic-hook installation, TLS-provider
initialization, desktop configuration, entering the event loop, and the first app
component render. `launch_returned` means the desktop launcher returned. A report
ending at a milestone identifies the last completed step; it does not establish
that step as the cause of the failure.

The shared Documents folder does not expose Library/Application Support, where
accounts and frontend settings remain. Existing private frontend logs stay in
Library. Native crashes, process termination, Objective-C exceptions and failures
before Rust starts may produce no Rust panic entry; these still require device or
Xcode diagnostics. No signal or exception handler is installed.

The vendored Tao state and class caches retain their original process lifetime
while using checked main-thread TLS instead of references to mutable statics.
RefCell callback exclusivity is preserved. This removes the identified source
warnings, but does not establish that those warnings caused a device launch crash.
A physical iPhone build and launch remain necessary to qualify the iOS-only code.
