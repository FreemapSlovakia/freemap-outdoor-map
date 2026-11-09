#!/usr/bin/env bash
# Expire cached tiles over a bounding box, so the renderer redraws them.
#
#   flush-tiles.sh <west> <south> <east> <north> [zoom]
#   flush-tiles.sh 5.63 34.31 21.09 47.99        # Italy
#
# Writes one z/x/y per line into the renderer's expires directory. The tile
# processing worker removes every descendant of each listed tile and every
# ancestor down to MAPRENDER_INVALIDATE_MIN_ZOOM, then redraws on demand, so
# listing a coarse zoom clears the whole pyramid beneath it. z10 is a good
# default: ~1 cache entry per 40 km, few enough lines to be quick, coarse
# enough that the ancestor walk reaches z8.
#
# The file must be written as `freemap`: the watcher runs as that user and
# deletes each file after processing it.
set -euo pipefail

if [ $# -lt 4 ]; then
  cat >&2 <<'USAGE'
usage: flush-tiles.sh <west> <south> <east> <north> [zoom]

  Expires cached tiles over a bounding box so the renderer redraws them.
  Coordinates are degrees; zoom defaults to 10.

  flush-tiles.sh 5.63 34.31 21.09 47.99      # Italy
  flush-tiles.sh 13.37 45.42 16.61 46.88     # Slovenia
  flush-tiles.sh 21.76 57.50 28.22 59.82     # Estonia

  HOST=fm5  EXPIRES=/home/freemap/X/active/expires   (override via environment)
USAGE
  exit 2
fi
W=$1; S=$2; E=$3; N=$4; Z=${5:-10}

if ! awk -v w="$W" -v s="$S" -v e="$E" -v n="$N" -v z="$Z" 'BEGIN{
      exit !(w<e && s<n && w>=-180 && e<=180 && s>=-85 && n<=85 && z>=0 && z<=18)}'; then
  echo "flush-tiles.sh: bad box or zoom (need west<east, south<north, |lat|<=85, 0<=zoom<=18)" >&2
  exit 2
fi
HOST=${HOST:-fm5}
EXPIRES=${EXPIRES:-/home/freemap/X/active/expires}

read -r X0 X1 Y0 Y1 < <(python3 - "$W" "$S" "$E" "$N" "$Z" <<'PY'
import math, sys
w, s, e, n, z = float(sys.argv[1]), float(sys.argv[2]), float(sys.argv[3]), float(sys.argv[4]), int(sys.argv[5])
m = 1 << z
def tx(lon): return int((lon + 180) / 360 * m)
def ty(lat):
    r = math.radians(lat)
    return int((1 - math.log(math.tan(r) + 1 / math.cos(r)) / math.pi) / 2 * m)
print(tx(w), tx(e), ty(n), ty(s))     # note: y grows southward
PY
)

tmp=$(mktemp)
for ((x = X0; x <= X1; x++)); do
  for ((y = Y0; y <= Y1; y++)); do
    printf '%d/%d/%d\n' "$Z" "$x" "$y"
  done
done > "$tmp"

count=$(wc -l < "$tmp")
stamp=$(date +%Y%m%d)/$(date +%H%M%S).000.tiles
echo "expiring $count z$Z tiles over $W,$S..$E,$N on $HOST"

# chmod: scp lands the file 0600 as the login user, and `freemap` then cannot
# read it to copy it into place.
scp -q "$tmp" "$HOST:/tmp/expire.tiles"
ssh "$HOST" "chmod 644 /tmp/expire.tiles \
          && sudo -u freemap mkdir -p $EXPIRES/$(dirname "$stamp") \
          && sudo -u freemap cp /tmp/expire.tiles $EXPIRES/$stamp \
          && rm -f /tmp/expire.tiles"
rm -f "$tmp"

echo "queued as $EXPIRES/$stamp — the watcher consumes it within a minute"
