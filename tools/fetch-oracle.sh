#!/usr/bin/env bash
# Fetches blargg's nes_ntsc 0.2.2 (the M1 decode oracle, LGPL 2.1) into
# crates/ntsc-oracle/vendor/, verified by hash. The canonical URL
# (blargg.parodius.com via slack.net/~ant/libs/ntsc.html) is dead; the
# Wayback Machine's 2011-08-12 capture of the canonical URL is the source,
# and the sha256 below pins exactly that capture.
set -euo pipefail
cd "$(dirname "$0")/.."

URL="http://web.archive.org/web/20110812132600if_/http://blargg.parodius.com/libs/nes_ntsc-0.2.2.zip"
SHA256="ca1a420d721d83b944142c366a917ba199dbc10cf91ad6f21dc712ed1069d58e"
DEST="crates/ntsc-oracle/vendor"

if [ -f "$DEST/nes_ntsc-0.2.2/nes_ntsc.c" ]; then
    echo "already fetched: $DEST/nes_ntsc-0.2.2"
    exit 0
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
curl -sL -o "$tmp/nes_ntsc-0.2.2.zip" "$URL"
echo "$SHA256  $tmp/nes_ntsc-0.2.2.zip" | sha256sum -c -
mkdir -p "$DEST"
python3 -c "import zipfile,sys; zipfile.ZipFile(sys.argv[1]).extractall(sys.argv[2])" \
    "$tmp/nes_ntsc-0.2.2.zip" "$DEST"
echo "fetched and verified: $DEST/nes_ntsc-0.2.2"
