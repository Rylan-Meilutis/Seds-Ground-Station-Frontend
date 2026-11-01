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

Recover Abort and Resync require an advertised capability, a fresh two-second
board readback and safe GS Startup/Idle. Reset ALL to Startup additionally
requires all seven boards' fresh boot/epoch/nonce identities and explicit
`capabilities.reset_startup == true`; query or resync support alone is insufficient.
Unsupported boards are listed and prevent a misleading global success claim.
Reset ALL cancels queued automation, turns AB supplies and igniter OFF, puts its
stepper to sleep, turns the VB pilot OFF and restores vent/dump OPEN. The in-app
confirmation explains this movement. It preserves network power, clock and
calibration. No control uses `window.confirm` or a native JavaScript dialog.

An operator first prepares a server-owned 30-second ticket. The DOM dialog shows
the exact targets, summary and confirmation phrase. Cancel sends no confirmation.
Before Confirm, the client fetches current status and checks backend, principal,
reset epoch, target boot/epoch/nonce, relevant state and capability context. It
also rechecks local station/session/permission and consumes the dialog once.
A changed context, failed status read or expired/non-monotonic ticket cancels the
request; it is never automatically retried. The server remains authoritative and
refreshes actual board readbacks before effects: an unchanged ticket identity
is not a claim that an older two-second output sample remains fresh for30seconds.

Request acceptance is displayed as pending, not safe-state proof. Failed,
cancelled and unconfirmed operations remain explicit. Active mode is read from
the server; selecting/requesting a mode never optimistically relabels the UI.
Only advertised runtime-supported modes are selectable. A compiled current-only
server displays the other modes as requiring the appropriate build/restart.

The browser fixture uses real Wasm with synthetic authenticated HTTP/WS responses
and no hardware outputs. It exercises confirmation/cancellation, identity changes,
legacy capabilities, all-board scope, mode honesty and mobile widths. It does not
qualify a real board's recovery adapter or physical valve movement.

Local status age includes the entire GET request/transport interval, conservatively
anchored to request start. Ticket age starts with preparation, so response delay
cannot extend the confirmation window. Non-finite or regressed local clocks fail
closed; no remote UTC epoch is compared with the client monotonic clock.
