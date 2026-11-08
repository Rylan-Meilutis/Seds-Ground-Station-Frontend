# Connectivity recovery controls

Connectivity contains per-board network health, diagnostics, actual board state
and explicit recovery controls. Detailed Info retains general network statistics.
Missing board state is **Unknown**, even when GS is in Startup. Healthy network
traffic does not establish an abort latch, safe output state or recovery support.
GPIO/servo command-register readback is not physical valve-position sensing.

GET `/api/board-recovery/status` polls local server state only. No page mount,
refresh, reconnect or status poll issues a board query or recovery operation.
An operator Query requires the current board's advertised dedicated recovery
endpoint and explicit query capability. Legacy/unknown endpoints remain disabled.
SD retry remains a separate explicit operator action.

Recover Abort requires an advertised capability, a fresh two-second board
readback and safe GS Startup/Idle/Aborted. Reset ALL to Startup additionally
requires all seven boards' fresh boot/epoch/nonce identities and explicit
`capabilities.reset_startup == true`; query or resync support alone is insufficient.
Unsupported boards are listed and prevent a misleading global success claim.
Reset ALL cancels queued automation, turns AB supplies and igniter OFF, puts its
stepper to sleep, turns the VB pilot OFF and restores vent/dump OPEN. The in-app
confirmation explains this movement. It preserves network power, clock and
calibration. No control uses `window.confirm` or a native JavaScript dialog.

**Resync** appears only on a board whose readback advertises it
(`capabilities.resync == true`, currently the Actuator and Valve Boards). The
button reads "Resync to <phase>…". It sets the board's state machine and phase
to the station's current phase without resetting it to Idle, and clears a
latched board Abort. It is allowed only in the fill phases and Armed. When it
is unavailable, the server's plain-language reason (`resync_blocked_reason`)
is shown as text under the buttons, for example a latched tank overpressure,
outputs that do not read safe, or "use Review recovery" in Idle. The review
dialog is titled "Resync <board> to <phase>", says that no outputs move, and
types the server phrase (`RESYNC AB TO NITROUSFILL`). It is accepted only when
the ticket's phase matches the station's current phase and carries
`outputs_unchanged: true`. Confirm re-prepares like every recovery action,
so a station that changed phase is refused rather than resynced to the old
phase. See GroundStation26 `docs/backend/board-resync.md`.

An operator first prepares a server-owned 30-second ticket. The DOM dialog shows
the exact targets, summary and confirmation phrase. Cancel sends no confirmation.
Before Confirm, the client fetches current status and checks backend, principal,
reset epoch, the server-owned ticket identity, exact targets and permissions. It
also rechecks local station/session/permission and consumes the dialog once.
Expiring two-second board readbacks do not cancel a still-valid ticket; they remain
displayed as stale. Revoking or consuming the server ticket cancels confirmation.
A changed context, failed status read or expired/non-monotonic ticket cancels the
request; it is never automatically retried. The server remains authoritative and
refreshes actual board readbacks before effects: an unchanged ticket identity
is not a claim that an older two-second output sample remains fresh for 30 seconds.

An uncommissioned board and retained first-identity uncertainty are shown
explicitly; neither counts as a completed reset. An incomplete earlier recovery
retains its target scope. Reset success requires the server to report that Startup
was applied; normal initialization can subsequently advance to Idle.

Request acceptance is displayed as pending, not safe-state proof. Failed,
cancelled and unconfirmed operations remain explicit. Operating mode selection
lives in Settings → Ground Station → Mission through the independent optional
[backend-defined mode contract](station-modes.md). Connectivity retains board
health, query, recovery and Startup reset details; it no longer hosts a duplicate
mode selector. A mode report never counts as board recovery or physical proof.

The browser fixture uses real Wasm with synthetic authenticated HTTP/WS responses
and no hardware outputs. It exercises confirmation/cancellation, identity changes,
legacy capabilities, all-board scope, mode honesty and mobile widths. It does not
qualify a real board's recovery adapter or physical valve movement.

Local status age includes the entire GET request/transport interval, conservatively
anchored to request start. Ticket age starts with preparation, so response delay
cannot extend the confirmation window. Non-finite or regressed local clocks fail
closed; no remote UTC epoch is compared with the client monotonic clock.
