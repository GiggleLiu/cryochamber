#!/usr/bin/env bash
# Capture a PNG of the cryohub dashboard.
#
# Usage:
#   screenshot.sh [--workspace DIR] [--chamber NAME|none] [--output PATH]
#                 [--size WxH] [--port N]
#
# Defaults:
#   --workspace examples/chambers
#   --chamber   mr-lazy
#   --output    docs/src/images/cryohub-dashboard.png
#   --size      1440x900
#   --port      18765
#
# Exits non-zero if prerequisites are missing or no PNG is written.

set -u

WORKSPACE="examples/chambers"
CHAMBER="mr-lazy"
OUTPUT="docs/src/images/cryohub-dashboard.png"
SIZE="1440x900"
PORT="18765"

while [ $# -gt 0 ]; do
  case "$1" in
    --workspace) WORKSPACE="$2"; shift 2 ;;
    --chamber)   CHAMBER="$2";   shift 2 ;;
    --output)    OUTPUT="$2";    shift 2 ;;
    --size)      SIZE="$2";      shift 2 ;;
    --port)      PORT="$2";      shift 2 ;;
    -h|--help)
      sed -n '2,14p' "$0" | sed 's/^# \{0,1\}//'
      exit 0 ;;
    *) echo "ERROR: unknown arg: $1" >&2; exit 2 ;;
  esac
done

# --- Prerequisites ---------------------------------------------------------
case "$(uname -s)" in
  Darwin)
    CHROME="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
    if [ ! -x "$CHROME" ]; then
      echo "ERROR: Google Chrome not found at $CHROME" >&2; exit 1
    fi ;;
  Linux)
    CHROME=""
    for c in google-chrome chromium chromium-browser; do
      if command -v "$c" >/dev/null 2>&1; then CHROME="$(command -v "$c")"; break; fi
    done
    if [ -z "$CHROME" ]; then
      echo "ERROR: no chrome/chromium found on PATH" >&2; exit 1
    fi ;;
  *) echo "ERROR: unsupported platform $(uname -s)" >&2; exit 1 ;;
esac

if ! command -v cryohub >/dev/null 2>&1; then
  echo "cryohub not on PATH; building..." >&2
  cargo build --quiet --bin cryohub || { echo "ERROR: cryohub build failed" >&2; exit 1; }
  CRYOHUB="$(pwd)/target/debug/cryohub"
else
  CRYOHUB="$(command -v cryohub)"
fi

if [ ! -d "$WORKSPACE" ]; then
  echo "ERROR: workspace dir does not exist: $WORKSPACE" >&2; exit 1
fi

# Parse size
W="${SIZE%x*}"
H="${SIZE#*x}"
if ! [[ "$W" =~ ^[0-9]+$ ]] || ! [[ "$H" =~ ^[0-9]+$ ]]; then
  echo "ERROR: --size must be WxH (e.g. 1440x900)" >&2; exit 2
fi

mkdir -p "$(dirname "$OUTPUT")"

# --- Launch cryohub --------------------------------------------------------
LOG="/tmp/cryohub-screenshot-$$.log"
USER_DIR="/tmp/chrome-shot-$$"
STARTED_CHAMBER=0

cleanup() {
  if [ "${STARTED_CHAMBER}" = "1" ] && [ -n "${CHAMBER_ID:-}" ]; then
    curl -fsS -X POST "http://127.0.0.1:$PORT/api/chambers/$CHAMBER_ID/stop" >/dev/null 2>&1 || true
  fi
  if [ -n "${HUB_PID:-}" ]; then
    kill "$HUB_PID" 2>/dev/null || true
  fi
  rm -rf "$USER_DIR" "$LOG"
}
trap cleanup EXIT

(
  cd "$WORKSPACE"
  "$CRYOHUB" start --foreground --port "$PORT" >"$LOG" 2>&1
) &
HUB_PID=$!

# Wait for hub to be reachable.
for _ in $(seq 1 60); do
  if curl -fsS "http://127.0.0.1:$PORT/" -o /dev/null 2>/dev/null; then break; fi
  if ! kill -0 "$HUB_PID" 2>/dev/null; then
    echo "ERROR: cryohub exited before becoming reachable. Log:" >&2
    cat "$LOG" >&2
    exit 1
  fi
  sleep 1
done
if ! curl -fsS "http://127.0.0.1:$PORT/" -o /dev/null 2>/dev/null; then
  echo "ERROR: cryohub never became reachable on port $PORT" >&2; exit 1
fi

# --- Resolve URL -----------------------------------------------------------
if [ "$CHAMBER" = "none" ] || [ -z "$CHAMBER" ]; then
  URL="http://127.0.0.1:$PORT/"
  CHAMBER_ID=""
else
  CHAMBER_ID="$(curl -fsS "http://127.0.0.1:$PORT/api/chambers" \
    | CH="$CHAMBER" python3 -c "import json,os,sys
cs=json.load(sys.stdin)
name=os.environ['CH']
hit=next((c for c in cs if c['name']==name), None)
if hit is None:
    print('NOT_FOUND', file=sys.stderr); sys.exit(3)
print(hit['id'])")"
  if [ -z "$CHAMBER_ID" ]; then
    echo "ERROR: chamber '$CHAMBER' not found in $WORKSPACE" >&2; exit 1
  fi
  URL="http://127.0.0.1:$PORT/c/$CHAMBER_ID"
  # Start it so the messages tab is populated.
  if curl -fsS -X POST "http://127.0.0.1:$PORT/api/chambers/$CHAMBER_ID/start" >/dev/null 2>&1; then
    STARTED_CHAMBER=1
    sleep 1
  fi
fi

# --- Headless Chrome screenshot --------------------------------------------
rm -rf "$USER_DIR"
mkdir -p "$USER_DIR"

"$CHROME" \
  --headless=old --disable-gpu --no-sandbox \
  --no-first-run --no-default-browser-check --disable-extensions \
  --disable-component-update --disable-background-networking \
  --hide-scrollbars \
  --user-data-dir="$USER_DIR" \
  --window-size="$W,$H" \
  --screenshot="$OUTPUT" \
  "$URL" >/dev/null 2>&1 || true

if [ ! -s "$OUTPUT" ]; then
  echo "ERROR: no screenshot produced at $OUTPUT" >&2
  echo "--- cryohub log ---" >&2
  cat "$LOG" >&2 || true
  exit 1
fi

BYTES="$(wc -c <"$OUTPUT" | tr -d ' ')"
echo "ok: wrote $OUTPUT ($BYTES bytes, ${W}x${H}, chamber=${CHAMBER})"
