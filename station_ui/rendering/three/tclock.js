// The one T-clock text format for station JS (replay players, programme pages).
// Same text as the Rust `tclock.rs` (frontend and backend) and the watch apps;
// watch/test-vectors.json `tclock_format` pins it.
//
//   MM:SS under an hour, H:MM:SS from one hour, D:HH:MM:SS from one day, with
//   an optional truncated fraction. The lead field never wraps. Before T-0 is
//   `T- `, from T-0 `T+ `. Missing, NaN or infinite input is `T --:--`.
//
// No imports and only top-level `export` declarations, so a self-contained
// classic script can inline it after stripping the `export ` keywords.
export const TCLOCK_UNKNOWN_BODY = '--:--';
export const TCLOCK_UNKNOWN = 'T --:--';

const pad2 = (n) => String(n).padStart(2, '0');

// Clock digits for a magnitude in ms; TCLOCK_UNKNOWN_BODY when not finite.
export function tclockBody(magnitudeMs, fractionDigits = 0) {
  if (typeof magnitudeMs !== 'number' || !Number.isFinite(magnitudeMs)) return TCLOCK_UNKNOWN_BODY;
  const digits = Math.max(0, Math.min(3, Math.trunc(Number(fractionDigits)) || 0));
  // BigInt keeps every digit exact (and out of exponent form) past 2^53.
  const ms = BigInt(Math.floor(Math.abs(magnitudeMs)));
  const totalS = ms / 1000n;
  const days = totalS / 86400n, hours = totalS / 3600n % 24n;
  const minutes = totalS / 60n % 60n, seconds = totalS % 60n;
  let text = days > 0n ? `${days}:${pad2(hours)}:${pad2(minutes)}:${pad2(seconds)}`
    : hours > 0n ? `${hours}:${pad2(minutes)}:${pad2(seconds)}`
    : `${pad2(minutes)}:${pad2(seconds)}`;
  if (digits > 0) text += '.' + String(ms % 1000n / 10n ** BigInt(3 - digits)).padStart(digits, '0');
  return text;
}

// Signed ms from T-0 (negative = before). `beforeT0` overrides the sign for a
// countdown that should still read `T- 00:00` at zero.
export function formatTClock(msFromT0, { fractionDigits = 0, beforeT0 } = {}) {
  if (typeof msFromT0 !== 'number' || !Number.isFinite(msFromT0)) return TCLOCK_UNKNOWN;
  const before = typeof beforeT0 === 'boolean' ? beforeT0 : msFromT0 < 0;
  return `T${before ? '-' : '+'} ${tclockBody(msFromT0, fractionDigits)}`;
}
