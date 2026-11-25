#!/bin/sh
# Makes a copy of a Proton install that runs a different wineserver.
#
# Usage: proton-with-wineserver.sh <proton dir> <wineserver> <new dir>
#
# Proton ignores WINESERVER: ntdll starts the server found next to the real
# path of ntdll.so. So the copy is a tree of symlinks in which ntdll.so, the
# loader, wine, the proton script and the server are real files.
set -eu
proton=$1 server=$2 out=$3
[ ! -e "$out" ] || { echo "$out exists" >&2; exit 1; }
cp -as "$(cd "$proton" && pwd)" "$out"
for f in proton files/bin/wine files/lib/wine/x86_64-unix/ntdll.so \
         files/lib/wine/x86_64-unix/wine-preloader files/lib/wine/x86_64-unix/wine; do
    if [ -e "$proton/$f" ]; then
        rm -f "$out/$f"
        cp "$proton/$f" "$out/$f"
    fi
done
rm -f "$out/files/bin/wineserver"
cp "$server" "$out/files/bin/wineserver"
chmod +x "$out/files/bin/wineserver"
echo "made $out; run the title with $out/proton"
