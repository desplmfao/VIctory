#!/bin/sh
# Builds a wineserver with the view-tree patch from the Wine source a Proton
# build was made from. The patch is ported to several trees
# (wineserver-view-tree-*.patch.txt); the first that applies is used.
#
# Usage: build-wineserver.sh <git url> <tag, branch or commit> <work dir> [<Proton dir>]
#    or: build-wineserver.sh <prepared Wine source dir> - <work dir> [<Proton dir>]
# With a Proton dir, the built server's protocol version is checked
# against that Proton's ntdll.so.
#        (a source tree prepared elsewhere, e.g. GE-Proton's, see docs/linux.md)
# Examples:
#   proton-cachyos 11.0: https://github.com/CachyOS/wine-cachyos.git cachyos-11.0-20260702-proton-native
#   Valve Proton 10:     https://github.com/ValveSoftware/wine.git proton_10.0
#   Proton Experimental: https://github.com/ValveSoftware/wine.git experimental_11.0 (the 11.0 patch)
#   upstream Wine:       https://gitlab.winehq.org/wine/wine.git master
# GE-Proton 11: its Wine is Valve's plus GE's patches, applied by GE's
# prep script; prepare that tree first and pass its directory (the 11.0
# patch applies to it).
#
# The server must speak the same protocol as the Proton it runs with: build
# from the source of that exact Proton release. The result is
# <work dir>/build/server/wineserver.
set -eu
url=$1 ref=$2 work=$3 proton=${4:-}
here=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$work"
if [ -d "$url" ]; then
    src=$(cd "$url" && pwd)
    mkdir -p "$work"
    cd "$work"
else
    cd "$work"
    if [ ! -d src ]; then
        if printf %s "$ref" | grep -Eq '^[0-9a-f]{40}$'; then
            # A commit, e.g. the Wine submodule of a Proton tag.
            git init -q src
            git -C src fetch -q --depth 1 "$url" "$ref"
            git -C src checkout -q FETCH_HEAD
        else
            git clone --depth 1 --branch "$ref" "$url" src
        fi
    fi
    src=$(pwd)/src
fi
cd "$src"
applied=
for p in "$here"/wineserver-view-tree-*.patch.txt; do
    if git apply --check --reverse "$p" 2>/dev/null; then
        applied=$p; break
    fi
    if git apply --check "$p" 2>/dev/null; then
        git apply "$p"; applied=$p; break
    fi
done
[ -n "$applied" ] || { echo "no view-tree patch applies to this tree" >&2; exit 1; }
echo "using $(basename "$applied")"
[ -x configure ] || ./autogen.sh
# The committed header can lag behind protocol.def (Valve's experimental
# tree says 930 where the requests make 931): regenerate it.
tools/make_requests
version=$(sed -n 's/^#define SERVER_PROTOCOL_VERSION //p' include/wine/server_protocol.h)
echo "SERVER_PROTOCOL_VERSION $version"
if [ -n "$proton" ]; then
    ntdll=$(find "$proton/files/lib" "$proton/dist/lib" -path '*x86_64-unix/ntdll.so' 2>/dev/null | head -1)
    if [ -z "$ntdll" ]; then
        echo "no x86_64-unix/ntdll.so under $proton: protocol not checked" >&2
    elif objdump -d --no-show-raw-insn "$ntdll" | grep -Eq "(cmp|mov)[a-z]* +\\\$0x$(printf %x "$version"),"; then
        echo "protocol $version matches $ntdll"
    else
        echo "protocol $version does not match $ntdll: build from the source of that exact Proton" >&2
        exit 1
    fi
fi
mkdir -p "$work/build"
cd "$work/build"
# Only the server is built, so nearly every library is left out.
"$src/configure" --enable-win64 --without-x --without-freetype --without-mingw \
    --without-gstreamer --without-wayland --without-vulkan --without-opengl \
    --without-alsa --without-pulse --without-cups --without-dbus --without-gnutls \
    --without-sane --without-usb --without-v4l2 --without-krb5 --without-netapi \
    --without-pcap --without-sdl --without-udev --without-unwind --without-capi \
    --without-gphoto --without-ffmpeg --disable-tests > configure.log
make -j"$(nproc)" server/wineserver > make.log
echo "built $(pwd)/server/wineserver"
