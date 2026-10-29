# Checklist sign-off

After completing every item, an administrator or a signed-in reviewer with the
**Sign off checklists** permission can approve the current checklist. The server
records who reviewed it and when. Item completion alone is not approval.

The review panel shows current and invalidated approvals. Any change to reviewed
contents or completion invalidates approval and requires a fresh review. If the
page is stale, signing is rejected until the latest version is reviewed.

Use **Export checklists** for the complete JSON document and **Export sign-offs
CSV** for the reviewer/time audit. With launch enforcement enabled, all required
checklists must be both complete and signed off. Signing does not operate valves
or run a procedure. Paired-station progress is shared; approvals remain local to
the reviewing station and are exported from that station.

Sign-off and audit exports require a station that supports them. When an older
station omits approval support, those controls stay unavailable and checklist
completion follows that station’s existing item-based policy.
