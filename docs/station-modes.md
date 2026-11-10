# Backend-defined runtime modes and Simulation

Settings → Ground Station presents policy mode and Simulation above its ordinary
configuration sections. These separately owned controls remain visible while
ordinary admissions are closed, so explicitly offered backend repair stays reachable.
Client settings stay per client/account. Calibration procedures stay in Calibration;
board recovery, network health and Startup reset stay in Connectivity.

A backend supplies opaque mode IDs, plain-text labels, and explicit supported
`(mode_id, simulation)` pair targets. The frontend never invents modes, board IDs,
mode combinations or output policy. The selected pair is a **draft**. Only actual
backend readback changes the reported mode. Simulation means incoming telemetry
and actions use an isolated simulated domain, with physical transport and outputs
excluded by the backend. Failure must not fall back to hardware.

## Optional V2 capability

`GET /api/station-modes` is an authenticated observation. See
[the custom backend example](api-examples/station-modes.custom.json). Version 2
requires `backend_instance`, `authority_id`, `principal_context`, `revision`,
`policy_epoch`, `authority_current`, `valid_for_ms`, `epoch_fencing_supported`,
`admissions_open`, `change_supported`, `cancel_supported`, `permissions`, `current`,
`modes`, `options`, `transition` and `reason`.

`current`, `policy_epoch`, `transition` and `reason` are explicitly nullable.
`current` reports `{id,mode_id,label,simulation,output_domain}` independently of
selectable options. Its label describes the whole pair; `modes[].label` labels the
mode dropdown. Each option adds `available`, nullable `reason` and
`transition_kind:"in_place"`. Every mode ID and option ID is unique; there is at
most one option per `(mode_id, simulation)`, and every referenced mode is supplied.
The current pair may be outside selectable options. Nothing implies a Cartesian
product of offered modes and execution domains.

One applied epoch within the same backend/authority binds one pair ID, mode ID and
Simulation value. Every actual commit, including same-pair repair, requires a new
epoch. Reusing an epoch for another identity is protocol incoherence: the client
retires ordinary work and its applied seal, marks status unverified, and requires a
genuinely new coherent epoch or authority lineage. Restoring the old descriptor
cannot rehabilitate that quarantined epoch. Label and description edits are not
identity changes. The client bounds quarantined identities and fails closed if
that safety history exhausts its capacity, rather than forgetting a bad identity.

An incomplete `current`/epoch never grants ordinary admission. After a known
ordinary context becomes incomplete, old intents cannot revive when that context
returns. A previously verified same-owner applied seal may still support matching
read-only data and explicit Stop under the separate retained-seal rules below;
this does not make the displayed execution domain verified or assume hardware.

IDs are opaque nonempty strings up to 256 UTF-8 bytes. Epochs are opaque strings
up to 512 bytes, qualified by the backend owner as well as its generation. Never
parse them as counters. Labels/output descriptions are plain text up to 512 bytes.
This client accepts at most 64 mode descriptors and 128 pair options. Observation
TTL is 1–30,000 ms, measured conservatively from request creation. Unknown,
malformed, expired and nonauthoritative observations grant no mode authority.

`permissions` contains `can_change` and `can_cancel_transition`. A truthful backend
keeps `change_supported:false` until in-place guards, IO retirement, persistence,
authorization and domain isolation are installed and qualified. The client also
keeps its integration gate false while ordinary producer migration is incomplete.
A descriptive mode label is not evidence that switching works.

## Prepare, explicit confirmation and readback

Preparation POSTs to `/api/station-modes/prepare`, binding the original
`backend_instance`, `authority_id`, `principal_context`, `revision`, `policy_epoch`,
`current_id` and `target_id`. It changes no outputs or state. The response is V2,
echoes that binding and supplies a one-use `ticket`, a `transition_id` known before
Confirm, the exact `current` and `target` descriptors, server-owned `summary`,
nullable `required_confirmation`, and `expires_in_ms` in 1–15,000 ms.

A themed in-app confirmation shows the server text. Nothing is typed: the
Confirm button is the confirmation. Confirm POSTs to `/api/station-modes/confirm`,
retaining the original binding, `ticket` and `transition_id`; `confirmation` is
empty, or the server's own phrase echoed back when an older backend still
issues `required_confirmation`. Changing mode or Simulation drafts
never sends a command. Changing backend, account, authority, epoch, revision or
target invalidates the old review; it never retargets a dialog.

Confirm rechecks current authorization and the exact ticket/context atomically.
Its V2 response echoes the original backend/authority/principal/source epoch and
current/target IDs, with `accepted:true` and the exact transition in `pending`
state. That receipt is **not applied**. Only fresh authoritative GET readback of
the same backend/authority/principal, exact transition and requested pair, with
`state:"applied"` and a new owner-qualified policy epoch establishes a reported
result. The client retains the prepared transition even if the response is lost.
It never automatically resends Confirm. Applied mode is not physical-output proof.

Cancellation is a separate capability and requires fresh server permission.
Its own exact pending identity is backend/authority/principal/revision plus
`transition_id`; it never silently reopens an old epoch. This UI does not expose a
Cancel-transition action until the backend and its cancellation path qualify.
Canceling an unsubmitted review is local and sends nothing.

## Original ownership and epoch fences

Mode observation/readback survives ordinary epoch/admission changes while
remaining bound to original authentication, URL, connection and view lifetime.
It does not log out, restart WebSockets, stop recordings or reset clocks. Ordinary
mutation/draft/press owners retire irreversibly when admissions close, their epoch
changes, or authority changes. Availability revision alone invalidates mode tickets,
not unrelated ordinary work. Renewed views create new drafts; they never rebind old
operations. Last-known information stays visibly unverified.

An ordinary domain-sensitive HTTP mutation carries the original
`X-Station-Policy-Epoch`; a WS control frame carries original `policy_epoch`.
Capture occurs synchronously at gesture/operation creation before the first async
poll. Never substitute a newer epoch at send time. The server retains normal
auth/permission checks and rejects missing/mismatched epochs before admission or
effects. Exact handler classification, not the HTTP verb or a guessed path prefix,
determines whether the epoch applies. Auth, client preferences, immutable recording
file imports and independently authorized history/media have separate authorities;
remote-board SD/network controls remain domain-sensitive.

Only backend-issued `ActionControl.epoch_fence:"emergency_stop"` selects the
separately authenticated explicit Stop path. Missing metadata defaults to
`ordinary`; names and labels confer no exception. Once fencing is supported, Stop
requires an original verified **applied** epoch/domain seal. It may survive ordinary
admissions closure and temporarily unavailable mode status while its original
authentication, backend and connection remain current. An actual applied epoch or
domain change retires that intent, including same-pair repair. The server performs
final installed-epoch/domain and permission checks; an old Simulation Stop never
retargets physical outputs. Without a previously verified seal, obtain fresh status
and require a new explicit gesture. There is no untagged supported-mode fallback,
auth bypass or automatic Abort privilege.

Mode-scoped incoming envelopes carry `epoch_scope:"mode"` and the ORIGINAL
producer `policy_epoch`, captured before queueing. After sticky fencing support,
missing or mismatched mode epochs are rejected. Matching applied-domain read-only
data may still update during Closing/FailedClosed; this does not reopen ordinary
action owners. Independent auth/mode observations and immutable history use their
separate authorities and must not impersonate current telemetry. Before displaying
a newly applied domain, retire queued old live data and current facts; preserve
archived recordings and their historical provenance. Native queues carry immutable
applied stamps and recheck at the final UI reducer. A Send data stamp alone grants
neither authentication nor IO authorization; its full original-owner predicate
requires the UI runtime context.

Live domain-sensitive GETs also capture their original applied read context before
creating the asynchronous request. They send its `X-Station-Policy-Epoch`; the
response must echo that original epoch and `X-Station-Epoch-Scope: mode` before
its live JSON can be accepted. Neither the client nor server may stamp a delayed
response with whichever epoch is current at completion. A reseed must contain one
homogeneous original live context; immutable multi-context recording history stays
under its separate recording authority and retains each sample's provenance.

Backend adapters must explicitly classify active telemetry, operational state,
board/readiness/plumbing facts and active action policy. Keep auth/mode observation,
persisted notifications/messages, immutable recordings/history and neutral media
independent. A mixed response must separate its session metadata from current-domain
facts instead of treating a surviving recording/session as a new live sample. An
`independent` tag cannot exempt a recognized live packet family. After support is
observed, malformed metadata, duplicate recognized epoch fields, missing scope and
mismatched epochs fail closed; packets never teach the client a new applied mode.
The current private implementation remains unactivated while backend producer
stamping, endpoint classification, and causal receive tests are incomplete.

Before any `epoch_fencing_supported:true` is observed for an authority, absent or
V1 read-only APIs preserve legacy fixed-mode use. After support is observed,
missing, expired or malformed capability must never enable untagged fallback.
Support is sticky per configured origin/authority lineage across WS reconnection,
auth refresh and backend restart. An explicitly different authority is a new
lineage. All requests stay on fixed relative routes of the selected backend;
redirects are rejected and no backend-provided URL receives credentials.

RRLS adapters must proxy or derive mode-scoped active facts from the **signed
primary source and its original epoch**. A local RRLS policy epoch or local physical
samples must never be relabeled as primary Simulation. Unsupported primary-view
read endpoints remain unavailable/unknown after fencing support; they do not fall
back to local hardware facts. Local auth, station connectivity metadata and
recording-session status have their own explicit independent authority. The
current `RecordingStatus`, notifications/messages and Warning/Error/AlertAckState
envelopes are mode-scoped current responses even when their records persist.
Retain recorder/session and immutable history caches across a mode change, clear
live popup/action lists, and reseed current envelopes under the new original
applied epoch. This never restarts the recorder or relabels historical records. The whole `/api/gse/status` live projection is
mode-scoped even though its envelope contains sequence-instance metadata: that
metadata never establishes the execution domain.
