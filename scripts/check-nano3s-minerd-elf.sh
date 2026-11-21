#!/bin/sh
# Fail a Nano 3S mujina-minerd that the device cannot start.
#
# Reference binaries (both statically linked, no PT_INTERP, no DT_NEEDED):
#   IxTech v5.1.0 mujina-minerd
#     sha256 eff96369cfe6c9ec185906a97dd972f05541c606a0e0b48e2c2625b1e15b0049
#   fork v5.1.0-multipool-test.3 mujina-minerd
#     sha256 e707eb334935c803af261f35e87e9f7cc635c9254f7b6649d3fe00ced49a1855
#
# A dynamic binary is allowed only when its interpreter is the Xuantie
# loader and every NEEDED entry is in that reference set. The reference
# set is empty, so any NEEDED entry fails. GLIBC_ symbol versions must
# be 2.33 or older.
set -eu
BIN="${1:-}"
if [ -z "$BIN" ] || [ "$BIN" = "-h" ]; then
  echo "usage: $0 <mujina-minerd>" >&2
  exit 2
fi
XTHEAD="/lib/ld-linux-riscv64xthead-lp64d.so.1"
fail=0

interp=$(readelf -l "$BIN" | sed -n 's/.*Requesting program interpreter:[[:space:]]*//p' | tr -d '[]')
echo "PT_INTERP: ${interp:-<none>}"
if [ -n "$interp" ] && [ "$interp" != "$XTHEAD" ]; then
  echo "FAIL: interpreter '$interp' is not $XTHEAD" >&2
  fail=1
fi

echo "NEEDED:"
needed=$(readelf -d "$BIN" 2>/dev/null | sed -n 's/.*Shared library: \[\(.*\)\]/\1/p' || true)
if [ -z "$needed" ]; then
  echo "  <none>"
else
  printf '%s\n' "$needed" | sed 's/^/  /'
  echo "FAIL: NEEDED library is outside the v5.1.0 / test.3 set (empty):" >&2
  printf '%s\n' "$needed" >&2
  fail=1
fi

vers=$(
  {
    readelf -V "$BIN" 2>/dev/null || true
    readelf -sW "$BIN" 2>/dev/null || true
  } | grep -o 'GLIBC_[0-9][0-9.]*' | sort -u || true
)
echo "GLIBC symbol versions:"
if [ -z "$vers" ]; then
  echo "  <none>"
  echo "max GLIBC: <none>"
else
  printf '%s\n' "$vers" | sed 's/^/  /'
  max=$(printf '%s\n' "$vers" | sed 's/^GLIBC_//' | awk '
    function pack(v,   a, n) {
      n = split(v, a, ".")
      return a[1] * 1000 + (n > 1 ? a[2] : 0)
    }
    { if (pack($0) > pack(best)) best = $0 }
    END { print best }
  ')
  echo "max GLIBC: $max"
  awk -v n="$max" 'BEGIN {
    split(n, a, ".")
    major = a[1] + 0
    minor = a[2] + 0
    if (major > 2 || (major == 2 && minor > 33)) exit 1
  }' || {
    echo "FAIL: GLIBC_$max is newer than 2.33" >&2
    fail=1
  }
fi

exit "$fail"
