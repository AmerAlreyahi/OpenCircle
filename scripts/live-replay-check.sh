#!/bin/sh
# Live replay benchmark (#44): the harness "talk" clip's frames through the live core with fake engines
# (src/live_core/tests.rs, a fake clock: deterministic). Fails if a patch is stale, an unchanged line is
# drawn again, a held line is missed, or the text -> drawn time (frames x 1000/60 ms) regresses past
# the limits below (the measured numbers + a margin). Raise a limit only on purpose, in the commit that
# changes the core's timing. Needs `cargo test` (release: a minute with the cache).
set -eu
MED_MAX=${MED_MAX:-500} P95_MAX=${P95_MAX:-800} MAX_MAX=${MAX_MAX:-800}
out=$(cargo test --release replay_of_the_talk_clip -- --nocapture 2>&1) || { echo "$out"; exit 1; }
line=$(echo "$out" | grep '^talk replay:') || { echo "$out"; echo "no 'talk replay:' line"; exit 1; }
echo "$line"
num() { echo "$line" | sed -n "s/.*$1 \([0-9]*\).*/\1/p"; }
drawn=$(echo "$line" | sed -n 's/^talk replay: \([0-9]*\) of .*/\1/p'); held=$(echo "$line" | sed -n 's/^talk replay: [0-9]* of \([0-9]*\) .*/\1/p')
bad=0
fail() { echo "FAIL: $1"; bad=1; }
[ "$(num stale)" -eq 0 ] || fail "stale $(num stale)"
[ "$(num redraws)" -eq 0 ] || fail "redraws $(num redraws)"
[ "$drawn" -ge "$held" ] || fail "drawn $drawn of $held"
[ "$(num median)" -le "$MED_MAX" ] || fail "median $(num median) ms > $MED_MAX"
[ "$(num p95)" -le "$P95_MAX" ] || fail "p95 $(num p95) ms > $P95_MAX"
[ "$(num max)" -le "$MAX_MAX" ] || fail "max $(num max) ms > $MAX_MAX"
exit $bad
