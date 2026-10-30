# Native connection and login storage

On Raspberry Pi/Linux, connection settings and preferences are in
`$XDG_DATA_HOME/gs26/storage.json`, or `~/.local/share/gs26/storage.json` when
XDG_DATA_HOME is not set to an absolute path. Remembered sessions are in the same
folder as `auth_session.json`. The location does not depend on the build version
or installation directory. Launching under a different account or data-directory
environment selects that account's separate store; use a consistent launch user
and environment across rebuilds.

Settings updates are serialized across threads and native app processes using
OS advisory locks on stable `.lock` sidecars. They are not PID files and need no
manual stale-lock cleanup: process exit releases the OS lock. Files are replaced
through same-directory temporaries, flushed before rename, and on Unix the parent
directory is flushed as well. Unix replacements use mode 0600. An interrupted
replacement does not truncate the previous complete file.

Invalid nonempty preference JSON is preserved rather than overwritten with an
empty map. A historical zero-byte settings file has no recoverable keys and can
be populated again by saving settings. Storage errors are recorded in the private
frontend log as an operation and OS error category, without values or tokens.

Signing out still clears the remembered session for that backend, including its
paired-token aliases. It leaves the backend address and other preferences in
place. Sessions remain scoped to the selected backend, and cached account data
never restores live command authority. Server expiry or revocation still requires
signing in again. Browser storage/profile configuration is separate from these
native files.
