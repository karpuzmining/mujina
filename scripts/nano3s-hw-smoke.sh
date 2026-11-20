#!/bin/sh
# Read-only by default. Change a safe setting, read it back, and restore it.
# Usage: scripts/nano3s-hw-smoke.sh <ip> [--reboot-check] [--psu-bypass] [--immersion] [--pool] [--firmware]
set -eu
IP="${1:-}"
shift || true
REBOOT=0
DO_BYPASS=0
DO_IMMERSION=0
DO_POOL=0
DO_FW=0
for arg in "$@"; do
  case "$arg" in
    --reboot-check) REBOOT=1 ;;
    --psu-bypass) DO_BYPASS=1 ;;
    --immersion) DO_IMMERSION=1 ;;
    --pool) DO_POOL=1 ;;
    --firmware) DO_FW=1 ;;
    *) echo "unknown flag: $arg" >&2; exit 2 ;;
  esac
done
if [ -z "$IP" ]; then
  echo "usage: $0 <ip> [--reboot-check] [--psu-bypass] [--immersion] [--pool] [--firmware]" >&2
  exit 2
fi
BASE="http://$IP"
pass=0
fail=0
ok() { pass=$((pass + 1)); echo "PASS  $1"; }
bad() { fail=$((fail + 1)); echo "FAIL  $1"; }
need() { command -v "$1" >/dev/null || { echo "need $1" >&2; exit 2; }; }
need curl
need jq

check_get() {
  name="$1"
  url="$2"
  if curl -fsS "$url" >/dev/null; then ok "$name"; else bad "$name"; fi
}

check_get system "$BASE/api/v0/system"
check_get pools "$BASE/api/v0/pools"
check_get psu "$BASE/api/v0/boards/nano3s-0/psu"
POW=$(curl -fsS "$BASE/api/v0/system" | jq -r '.power.power_sensor // "missing"')
echo "power_sensor=$POW"

THEME=$(curl -fsS "$BASE/api/v0/display" | jq -r '.theme')
ALT=minimal
if [ "$THEME" = "minimal" ]; then ALT=cyberpunk; fi
CODE=$(curl -sS -o /tmp/mujina-smoke-body -w '%{http_code}' -X PATCH "$BASE/api/v0/display" \
  -H 'Content-Type: application/json' -d "{\"theme\":\"$ALT\"}")
if [ "$CODE" = "200" ]; then
  BACK=$(curl -fsS "$BASE/api/v0/display" | jq -r '.theme')
  if [ "$BACK" = "$ALT" ]; then ok "display theme change"; else bad "display theme readback ($BACK)"; fi
else
  bad "display theme patch HTTP $CODE"
fi
curl -fsS -X PATCH "$BASE/api/v0/display" -H 'Content-Type: application/json' \
  -d "{\"theme\":\"$THEME\"}" >/dev/null && ok "display theme restored" || bad "display theme restore"

if [ "$DO_BYPASS" = "1" ]; then
  echo "PSU bypass flag set; not changing it automatically in this script"
fi
if [ "$DO_IMMERSION" = "1" ]; then
  echo "immersion flag set; not changing it automatically in this script"
fi
if [ "$DO_POOL" = "1" ]; then
  echo "pool flag set; this script does not rewrite pool URLs"
fi
if [ "$DO_FW" = "1" ]; then
  echo "firmware flag set; this script does not upload firmware"
fi

if [ "$REBOOT" = "1" ]; then
  curl -fsS -X PATCH "$BASE/api/v0/display" -H 'Content-Type: application/json' \
    -d '{"theme":"parasite"}' >/dev/null
  curl -fsS -X POST "$BASE/api/v0/reboot" >/dev/null || true
  i=0
  while [ "$i" -lt 60 ]; do
    if curl -fsS "$BASE/api/v0/display" | jq -e '.theme == "parasite"' >/dev/null; then
      ok "theme survived reboot"
      break
    fi
    i=$((i + 1))
    sleep 5
  done
  if [ "$i" -ge 60 ]; then bad "theme after reboot"; fi
  curl -fsS -X PATCH "$BASE/api/v0/display" -H 'Content-Type: application/json' \
    -d "{\"theme\":\"$THEME\"}" >/dev/null || true
fi

echo "summary pass=$pass fail=$fail"
[ "$fail" -eq 0 ]
