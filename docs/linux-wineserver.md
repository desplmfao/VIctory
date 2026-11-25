# Linux: building the patched wineserver (from #2082)

Kept in this fork after the upstream pull request was slimmed down: the
patches are ajsem1337's view-tree change from
[#340](https://github.com/Marvisak/VIctory/issues/340), ported to several
Wine trees, with tests and contributions by neonbyte1 (each file credits
them). The scripts are in `tools/wine/`.

VIctory is a Windows program; on Linux it runs under Wine, in practice a
Proton build with vkd3d-proton for Direct3D 12. This is not a supported
platform. These notes are what has worked for testers.

## Setup

- A Proton build with vkd3d-proton (tested: proton-cachyos 11.0, with the
  `d3d12.dll` and `d3d12core.dll` of Proton Experimental 11.0 next to
  `victory.exe`, which report themselves as vkd3d-proton 3.2.0, build
  `2576c0b4`; proton-cachyos 11.0's own vkd3d-proton is an older build,
  `3dfc6f07`, and has not been measured here; GE-Proton 11 is reported to
  work too). Run `victory.exe` with `proton runinprefix` in a prefix of
  its own. A Proton's vkd3d-proton build is in
  `files/lib/wine/vkd3d-proton/version`.
- Use the native Direct3D 12 DLLs:
  `WINEDLLOVERRIDES="d3d12,d3d12core,dxgi=n,b"`, with vkd3d-proton's
  `d3d12.dll` and `d3d12core.dll` next to `victory.exe`. Wine's builtin
  `libvkd3d-1.dll` takes a different path and is not what has been tested.
- Use Microsoft's `d3dcompiler_47.dll`, not Wine's. VIctory compiles a few
  helper shaders of its own at run time with `D3DCompile`. Wine's builtin
  compiler refuses them (`E5005: Method 'Load' is not defined on type
  'RWByteAddressBuffer'`), and the log shows `no argument clamp: indirect
  execution dropped`: every indirect draw and dispatch is skipped, and most
  of the world is drawn that way. Put a 64-bit `d3dcompiler_47.dll` from
  Microsoft (for example from `winetricks d3dcompiler_47`, or the Windows
  SDK) next to `victory.exe` and add `d3dcompiler_47=n,b`:
  `WINEDLLOVERRIDES="d3d12,d3d12core,dxgi,d3dcompiler_47=n,b"`. Wine's own
  copy is about 400 KB; Microsoft's is about 4 MB.
- NVIDIA: compute `72f552f4553fb5f9` (first run at the start of the first
  mission, around frame 950) does not finish with vkd3d-proton's raw access
  chains: the kernel logs `NVRM: Xid 109 ... CTX SWITCH TIMEOUT`, the
  desktop stalls, and the title's own watchdog ends it (`Unsignaled
  CrossQueueBarrier` in its crash log, under `T:`). Set
  `VICTORY_SKIP_PIPELINES=72f552f4553fb5f9`: that one pass is dropped and
  the rest is drawn as before. `VKD3D_DISABLE_EXTENSIONS=VK_NV_raw_access_chains`
  also avoids the hang, but it changes how every shader is compiled, and
  here the picture turned white with it. RTX 5080, driver 615.71; see also
  #844 and #1528.
- NVIDIA: some translated shaders crash the driver's compiler, which ends
  the process. #491 (not merged yet) adds `VICTORY_PIPELINE_JOURNAL=1`:
  a pipeline whose creation ended a run is refused from the next run on,
  so each run gets further. Without #491 the variable does nothing.

## The wineserver patch (about four times faster)

Under Wine, much of a frame's time goes into the wineserver, which keeps
each process's mapped views in a list and walks it on every memory query.
VIctory asks many such questions per frame. ajsem1337's patch in #340, with
tests and contributions by neonbyte1, keeps the views in a tree instead. On an RTX 5080 with proton-cachyos 11.0 a
world frame took about 4.0 s with the stock server and about 1.0 s with
the patched one; testers in #340 report similar gains.

ajsem1337 has proposed the change to Wine itself
([Wine bug 60868](https://bugs.winehq.org/show_bug.cgi?id=60868)) and to
wine-cachyos ([CachyOS/wine-cachyos#50](https://github.com/CachyOS/wine-cachyos/pull/50)).
Once a Proton release ships it (for proton-cachyos, a release after that
pull request is merged; check its release notes), that Proton's own
wineserver already keeps views in a tree and none of the following is
needed. Until then, `tools/wine/` builds the patched server and makes a
Proton copy that uses it:

Two steps: build the server from the Wine source of the exact Proton you
run, then make a copy of that Proton that uses it. Pick your Proton:

**proton-cachyos 11.0** (tested with the title):

```sh
tools/wine/build-wineserver.sh https://github.com/CachyOS/wine-cachyos.git \
    cachyos-11.0-20260702-proton-native ~/wineserver-build   # the tag of your release
tools/wine/proton-with-wineserver.sh /usr/share/steam/compatibilitytools.d/proton-cachyos-native \
    ~/wineserver-build/build/server/wineserver ~/proton-victory
```

**Proton Experimental** (Steam's "Proton - Experimental"):

```sh
P=~/.steam/steam/steamapps/common/"Proton - Experimental"
# the Wine commit of the installed release (its tag is in "$P/version")
wine=$(gh api "repos/ValveSoftware/Proton/contents/wine?ref=experimental-11.0-20261112" --jq .sha)
tools/wine/build-wineserver.sh https://github.com/ValveSoftware/wine.git "$wine" \
    ~/wineserver-build "$P"
tools/wine/proton-with-wineserver.sh "$P" \
    ~/wineserver-build/build/server/wineserver ~/proton-victory
```

Experimental moves often. Build from the Wine commit of the release you
have installed (the Proton tag pins it), not the head of the
`experimental_11.0` branch, and build again after Steam updates it.
Tested on an RX 9070 (RADV) with `experimental-11.0-20261112`: a world
frame went from 2.9–3.2 s to 0.7–1.0 s (measured by Beqren, #2082).

**Valve Proton 10** (Steam's "Proton 10.0"):

```sh
tools/wine/build-wineserver.sh https://github.com/ValveSoftware/wine.git \
    proton_10.0 ~/wineserver-build
tools/wine/proton-with-wineserver.sh ~/.steam/steam/steamapps/common/"Proton 10.0" \
    ~/wineserver-build/build/server/wineserver ~/proton-victory
```

**GE-Proton 11** (e.g. GE-Proton11-14): GE's Wine is not in a single
repository. It is Valve's Wine with GE's patches, which GE's own script
applies. Prepare that source first, then pass its `wine` directory
instead of a git URL (`-` for the tag):

```sh
git clone --recurse-submodules --branch GE-Proton11-14 \
    https://github.com/GloriousEggroll/proton-ge-custom ~/proton-ge-src   # your release's tag
cd ~/proton-ge-src && ./patches/protonprep-valve-staging.sh &> patchlog.txt && cd -
tools/wine/build-wineserver.sh ~/proton-ge-src/wine - ~/wineserver-build
tools/wine/proton-with-wineserver.sh ~/.steam/steam/compatibilitytools.d/GE-Proton11-14 \
    ~/wineserver-build/build/server/wineserver ~/proton-victory
```

The patch applies to GE's Wine as GE's script leaves it, but this path
has not been built end to end yet.

**Upstream Wine** (a Wine you built or installed yourself, not Proton):
build from the same source and version as your Wine, e.g.
`https://gitlab.winehq.org/wine/wine.git` at its tag, and replace that
Wine's `wineserver` with the one built here.

Then run `victory.exe` with `~/proton-victory/proton runinprefix ...`.

- The server must come from the same Wine source as the Proton it runs
  with: the two speak a private protocol, and a mismatch fails at start-up.
  The build regenerates `server_protocol.h` (`tools/make_requests`; a
  committed header can lag behind, as Valve's experimental tree's does)
  and prints `SERVER_PROTOCOL_VERSION`. Given the Proton directory as a
  fourth argument, it also checks that version against that Proton's
  `ntdll.so` and stops on a mismatch.
- Instead of `proton-with-wineserver.sh`, a full copy of the Proton
  directory (`cp -a --reflink=auto`) with only `files/bin/wineserver`
  replaced works too: every file in it is real, so ntdll finds the new
  server.
- Proton ignores `WINESERVER`; ntdll starts the server found next to the
  real path of `ntdll.so`. That is why the copy is a tree of symlinks in
  which `ntdll.so`, the loader, `wine`, the `proton` script and the server
  are real files.
- Only one wineserver serves a prefix. Do not run anything else in the
  prefix with another Proton while the title runs: the first process
  started picks the server, and a run on the stock server is back to the
  old frame times without any warning.
- The patch is ported to these trees; the script uses the first that
  applies, and says which:

  | Proton / Wine | source to build from | patch |
  |---|---|---|
  | proton-cachyos 11.0 | `CachyOS/wine-cachyos`, tag of your release | `-cachyos11` |
  | Proton Experimental (11.0) | `ValveSoftware/wine`, `experimental_11.0` | `-cachyos11` |
  | GE-Proton 11 | `proton-ge-custom` at your release tag, after GE's prep script (see above) | `-cachyos11` |
  | Valve Proton 10 | `ValveSoftware/wine`, `proton_10.0` | `-proton10` |
  | upstream Wine | `gitlab.winehq.org/wine/wine`, `master` | `-upstream` |

  Each port builds without warnings and was checked against the original
  lookups by a randomized comparison; only proton-cachyos 11.0 has been
  run with the title. It is Wine code (LGPL-2.1-or-later), credit to
  ajsem1337 and neonbyte1.
