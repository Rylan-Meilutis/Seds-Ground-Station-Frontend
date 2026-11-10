#!/usr/bin/env bash
# Run every browser test in tests/ against freshly built bundles.
#
#   scripts/run-browser-suite.sh [--media] [--jobs N] [--skip-build] [--gs PATH]
#
# Builds the release web bundle and the preview examples, produces the inputs
# some tests read (Rust-exported replay HTML, the GroundStation display-history
# fixture, a chamber layout), serves the previews on the ports the tests
# expect, runs the tests and tears everything down. Synthetic data only; no
# station, radio or hardware is contacted.
#
# --media also starts the isolated media fixture (Docker MediaMTX, two
# synthetic ffmpeg cameras, GroundStation's serve_synthetic_media and a
# debugging Chrome) and runs the broadcast, model and crew-voice tests. Without
# it those five tests are listed as skipped. It needs Docker, ffmpeg and the
# loopback ports 18554, 19090, 19101, 19997 and 19998.
#
# Environment: PLAYWRIGHT_MODULE (path to playwright's index.mjs) and
# CHROME_BINARY (a Chrome/Chromium executable) are required.
set -uo pipefail

ui=$(cd "$(dirname "$0")/.." && pwd)
gs="$ui/../GroundStation26"
jobs=3
media=0
build=1
while [ $# -gt 0 ]; do
  case "$1" in
    --media) media=1 ;;
    --jobs) jobs=$2; shift ;;
    --skip-build) build=0 ;;
    --gs) gs=$2; shift ;;
    -h|--help) sed -n '2,20p' "$0"; exit 0 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
  shift
done
gs=$(cd "$gs" && pwd) || { echo "GroundStation26 checkout not found; pass --gs PATH" >&2; exit 2; }
: "${PLAYWRIGHT_MODULE:?set PLAYWRIGHT_MODULE to the index.mjs of a Playwright install}"
: "${CHROME_BINARY:?set CHROME_BINARY to a Chrome or Chromium executable}"
export PLAYWRIGHT_MODULE CHROME_BINARY

work=$(mktemp -d "${TMPDIR:-/tmp}/gs-browser-suite.XXXXXX")
results="$work/results"
mkdir -p "$results"
pids=()
container=""
cleanup() {
  # Each helper runs in its own session; stop the whole group (cargo's test
  # binary, ffmpeg, Chrome's children), not just its leader.
  for pid in "${pids[@]}"; do kill -- "-$pid" 2>/dev/null || kill "$pid" 2>/dev/null; done
  [ -n "$container" ] && docker stop "$container" >/dev/null 2>&1
  wait 2>/dev/null
}
trap cleanup EXIT INT TERM
step() { printf '\n== %s\n' "$*"; }
bg() { setsid "$@" </dev/null & pids+=($!); }
# bg_log LOG CMD...: like bg, with output kept out of the terminal.
bg_log() { local log=$1; shift; setsid "$@" </dev/null >"$log" 2>&1 & pids+=($!); }

cd "$ui"
dx_out="$ui/target/dx"
release="$dx_out/groundstation_frontend/release/web/public"
previews=(paired_settings_preview station_positions_preview antenna_preview network_health_preview sd_health_preview offline_preview)
if [ "$build" = 1 ]; then
  step "Building the release web bundle"
  dx build --platform web --release > "$work/build-release.log" 2>&1 || { tail -20 "$work/build-release.log"; exit 1; }
  for example in "${previews[@]}"; do
    step "Building preview $example"
    dx build --platform web --example "$example" > "$work/build-$example.log" 2>&1 || { tail -20 "$work/build-$example.log"; exit 1; }
  done
fi
preview() { echo "$dx_out/$1/debug/web/public"; }

step "Producing test inputs"
inputs="$work/inputs"
mkdir -p "$inputs"
GS_REPLAY_HTML_PROOF="$inputs/replay.html" GS_REPLAY_INVERTED_HTML_PROOF="$inputs/replay-inverted.html" \
  cargo test -- actual_offline_html_export_contains actual_inverted_valve_export --test-threads=1 > "$work/replay-proofs.log" 2>&1 \
  || { tail -20 "$work/replay-proofs.log"; exit 1; }
(cd "$gs" && GS_DISPLAY_HISTORY_FIXTURE="$inputs/history.json" cargo test -p groundstation_backend display_history_tests) \
  > "$work/history-fixture.log" 2>&1 || { tail -20 "$work/history-fixture.log"; exit 1; }
export GS_UI_BUNDLE="$release" GS_FRONTEND_PUBLIC="$release"
export GS_REPLAY_HTML_PROOF="$inputs/replay.html" GS_REPLAY_INVERTED_HTML_PROOF="$inputs/replay-inverted.html"
export GS_HISTORY_FIXTURE="$inputs/history.json" GS_CHAMBER_LAYOUT="$gs/backend/layout/layout.json"
export GS_PAIRED_WEB_ROOT="$(preview paired_settings_preview)" GS_STATION_POSITIONS_BUNDLE="$(preview station_positions_preview)"
export GS_UI_ARTIFACTS="$work/artifacts"
mkdir -p "$GS_UI_ARTIFACTS"

step "Serving the previews"
bg node scripts/serve-static.mjs "$(preview antenna_preview)" 19091
bg node scripts/serve-static.mjs "$(preview network_health_preview)" 19094
bg node scripts/serve-static.mjs "$(preview offline_preview)" 19096
bg node scripts/serve-static.mjs "$(preview sd_health_preview)" 19097

media_tests="station-broadcast.cjs station-model-animation.cjs concurrent-broadcast-browser.mjs station-voice_browser.mjs station-voice_join_failure.mjs"
if [ "$media" = 1 ]; then
  step "Starting the isolated media fixture"
  fixture="$work/media"
  mkdir -p "$fixture/recordings/test_camera" "$fixture/models"
  container="gs-browser-suite-$$"
  docker run -d --rm --name "$container" \
    -p 127.0.0.1:18554:8554 -p 127.0.0.1:19997:9997 -p 127.0.0.1:19998:8888 \
    -v "$gs/backend/config/mediamtx.yml:/mediamtx.yml:ro" \
    -e MTX_WEBRTCADDITIONALHOSTS=127.0.0.1 \
    -e MTX_AUTHINTERNALUSERS_0_USER=camera -e MTX_AUTHINTERNALUSERS_0_PASS=test-only \
    -e MTX_AUTHINTERNALUSERS_0_PERMISSIONS_0_ACTION=publish \
    -e MTX_AUTHINTERNALUSERS_1_USER=groundstation -e MTX_AUTHINTERNALUSERS_1_PASS=test-only \
    -e MTX_AUTHINTERNALUSERS_1_PERMISSIONS_0_ACTION=read \
    -e MTX_AUTHINTERNALUSERS_1_PERMISSIONS_1_ACTION=api \
    bluenviron/mediamtx:1.21.0 >/dev/null || exit 1
  sleep 3
  # concurrent-broadcast decodes camera audio, so the cameras publish AAC too.
  for path in front side; do
    source=testsrc2; [ "$path" = side ] && source=smptebars
    bg_log "$fixture/ffmpeg-$path.log" ffmpeg -hide_banner -loglevel error -re -f lavfi -i "$source=size=640x360:rate=15" \
      -re -f lavfi -i sine=frequency=440:sample_rate=48000 \
      -c:v libx264 -preset ultrafast -tune zerolatency -profile:v baseline -pix_fmt yuv420p -g 15 -bf 0 \
      -c:a aac -b:a 64k -f rtsp -rtsp_transport tcp "rtsp://camera:test-only@127.0.0.1:18554/$path"
  done
  # The crew-voice tests need one recording; give it a settled modification time.
  ffmpeg -hide_banner -loglevel error -f lavfi -i testsrc2=size=320x240:rate=15 -f lavfi -i sine=frequency=440 -t 4 \
    -c:v libx264 -pix_fmt yuv420p -c:a aac -y "$fixture/recordings/test_camera/2026-09-17_12-00-00-000001.mp4"
  touch -d '-10 seconds' "$fixture/recordings/test_camera/2026-09-17_12-00-00-000001.mp4"
  bg_log "$fixture/fixture.log" env -C "$gs" GS_MEDIA_TEST_DIR="$fixture" GS_STAGE_MODELS_DIR="$fixture/models" \
    GS_VIDEO_RECORDINGS_DIR="$fixture/recordings" GS_VIDEO_API_URL=http://127.0.0.1:19997 \
    GS_VIDEO_HLS_URL=http://127.0.0.1:19998 GS_VIDEO_PASSWORD=test-only \
    cargo test -p groundstation_backend media::tests::serve_synthetic_media -- --ignored --nocapture
  for _ in $(seq 120); do grep -q GS_MEDIA_TEST_LISTEN "$fixture/fixture.log" 2>/dev/null && break; sleep 5; done
  grep -q GS_MEDIA_TEST_LISTEN "$fixture/fixture.log" || { tail -20 "$fixture/fixture.log"; exit 1; }
  # Newer Chrome blocks the model test's data: page from loading loopback
  # scripts (private network access); this isolated profile allows it.
  bg_log "$fixture/chrome.log" "$CHROME_BINARY" --headless --remote-debugging-port=19101 --user-data-dir="$fixture/chrome" \
    --autoplay-policy=no-user-gesture-required --use-gl=swiftshader --enable-unsafe-swiftshader --ignore-gpu-blocklist \
    --disable-features=BlockInsecurePrivateNetworkRequests,PrivateNetworkAccessRespectPreflightResults,PrivateNetworkAccessSendPreflights,LocalNetworkAccessChecks \
    about:blank
  export GS_MEDIA_TEST_DIR="$fixture" GS_MEDIA_TEST_ORIGIN=http://127.0.0.1:19090 GS_BROWSER_TEST_ORIGIN=http://127.0.0.1:19090
  echo "Waiting 30 s for media retention"
  sleep 30
fi

step "Running browser tests ($jobs at a time)"
tests=()
for file in tests/*.mjs tests/*.cjs; do
  name=$(basename "$file")
  if [ "$media" = 0 ] && [[ " $media_tests " == *" $name "* ]]; then echo "skip $name" >> "$results/summary"; continue; fi
  # The media tests share one fixture and browser; run them one at a time below.
  if [ "$media" = 1 ] && [[ " $media_tests " == *" $name "* ]]; then continue; fi
  tests+=("$file")
done
printf '%s\n' "${tests[@]}" | xargs -P "$jobs" -I{} sh -c \
  'n=$(basename "{}"); if timeout 600 node "{}" > "'"$results"'/$n.log" 2>&1; then echo "pass $n"; else echo "FAIL $n"; fi >> "'"$results"'/summary"'
if [ "$media" = 1 ]; then
  for name in $media_tests; do
    if timeout 600 node "tests/$name" > "$results/$name.log" 2>&1; then echo "pass $name"; else echo "FAIL $name"; fi >> "$results/summary"
  done
fi

step "Results (logs in $results)"
sort -k2 "$results/summary"
printf '\n%s passed, %s failed, %s skipped\n' "$(grep -c '^pass' "$results/summary")" \
  "$(grep -c '^FAIL' "$results/summary")" "$(grep -c '^skip' "$results/summary")"
! grep -q '^FAIL' "$results/summary"
