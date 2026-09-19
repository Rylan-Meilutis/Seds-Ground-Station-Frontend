# Acceleration graph screening

`ACCEL_DATA` charts use display-only screening before chart bucketing. Firmware,
latest sensor values, stored recordings, and CSV exports are not modified.

The screen uses only three preceding samples, a median and median
absolute deviation (MAD), and a 0.1 m/s² minimum threshold. Samples exceeding
six robust standard deviations are checked for neighboring same-direction support
and simultaneous excursions on other accelerometer axes. This is a heuristic,
not a physical validity determination or a cross-sensor fusion algorithm.

- NaN and infinity are omitted, not replaced with zero.
- Supported peaks are retained.
- Unsupported finite impulses are ambiguous: they remain visible and increment
  the graph's ambiguous-impulse count. A genuine one-axis impulse cannot safely
  be distinguished from a faulty finite sample using these data alone.
- Missing context does not authorize rejection. Screening history resets across
  receipt gaps longer than two seconds.

The graph caption reports ambiguous impulses and omitted invalid values for the
current chart-cache lifetime. Every sample is returned immediately, including
the first and last: there is no look-ahead delay or pending tail. Transport and
render latency still apply. An event onset may be flagged because future support
is not yet available; a flag never removes a finite peak. Counts describe
suspect samples at arrival, not confirmed sensor faults.

After screening, acceleration render buckets retain first, last, and each axis's
minimum/maximum sample in chronological order (at most eight points for three
axes). No averaging or curve smoothing is applied to acceleration render paths.
Points in one downsampled column share its x coordinate; the extrema remain
visible. Other telemetry charts retain their existing rendering behavior.

Tests cover reordered batches, constant signals, real sustained peaks, ambiguous
single-sample impulses, multi-axis support, invalid/missing values, bounded
screening memory, and preservation of peaks during render downsampling.
