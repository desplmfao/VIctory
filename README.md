# VIctory

Runs Grand Theft Auto VI (Xbox Series X|S) natively on Windows, from files
the user supplies.

The title's x86-64 code runs unchanged inside our process; VIctory supplies
the platform underneath it and converts the title's data, ahead of time where
it can and on first use where it must (shaders). See
[docs/architecture.md](docs/architecture.md).

## This fork: the PRs we run (Linux, NVIDIA)

This fork's `main` is upstream `main` plus a stack of open pull requests that we
test together on Linux (CachyOS, proton-cachyos 11.0, vkd3d-proton, RTX 5080).
Upstream only takes pull requests from collaborators now, so this branch keeps
track of what we use. Clone it and build as described below.

Upstream base: Marvisak/VIctory `main` at `ffe310e`.

| PR | Author | What it does |
|----|--------|--------------|
| [#185](https://github.com/Marvisak/VIctory/pull/185) via [aycow/VIctory#4](https://github.com/aycow/VIctory/pull/4) | aycow (merge with main: desplmfao) | Shadow-buffer coherency for directly indexed SRVs; includes [aycow/VIctory#2](https://github.com/aycow/VIctory/pull/2) (shared write-watch questions) |
| [#493](https://github.com/Marvisak/VIctory/pull/493) | hophaver | Root SRVs as one-view tables; fixes the hang at world entry |
| [#259](https://github.com/Marvisak/VIctory/pull/259) | desplmfao | Query a descriptor's level count only when word 3 is read (NVIDIA pipeline crash) |
| [#178](https://github.com/Marvisak/VIctory/pull/178) | Ortomanic | Compile the indirect-argument clamp with DXC when D3DCompile refuses it (Wine/Proton) |
| [#151](https://github.com/Marvisak/VIctory/pull/151) | ItsYusei99 | 16 MiB default thread stack |
| [#542](https://github.com/Marvisak/VIctory/pull/542) | gingertonik | Keep buffer offsets below 2 GiB (NVIDIA device loss in the world) |
| [#180](https://github.com/Marvisak/VIctory/pull/180) | permadiego | Twin descriptor loads and quad gradients for compute samples (+300 compute shaders) |
| [#539](https://github.com/Marvisak/VIctory/pull/539) | SAitken91 | 64-bit image atomics (+522 vertex/pixel shaders) |
| [#551](https://github.com/Marvisak/VIctory/pull/551) | kilvynsilveira-tech | Compute group info, wave id in workgroup, sample coverage |
| [#489](https://github.com/Marvisak/VIctory/pull/489) | Brazlucas | Spill-table loads read the arguments as they were on entry |
| [#177](https://github.com/Marvisak/VIctory/pull/177) (commit `e55eb69` only) | Ortomanic | Key heap handles by element type; without it NVIDIA crashes creating several compute pipelines |
| [#491](https://github.com/Marvisak/VIctory/pull/491) | desplmfao | `VICTORY_PIPELINE_JOURNAL`: skip pipelines whose creation crashed an earlier run |
| [#638](https://github.com/Marvisak/VIctory/pull/638) | claude-GTA6 | Diagnostics: render-target capture and display (`VICTORY_CAPTURE*`, `VICTORY_SHOW_TARGET`) |

Local addition, for testing only: `VICTORY_CAPPED_DYNAMIC=16..19` sets how many
dynamic descriptor slots the capped NVIDIA view heap keeps (default 16, that is
2^16). 18 removes the "no dynamic slots" draw drops; see issue
[#747](https://github.com/Marvisak/VIctory/issues/747).

Running under Proton is much faster with the wineserver patch from
[#340](https://github.com/Marvisak/VIctory/issues/340) (about 4 s to 1 s per
world frame here).

State of this stack (2026-11-24): boots and reaches the world at about
1.5 fps and 4,000 draws per frame. The picture is mostly white, which earlier
stacks without #180/#539 did not show; this is being investigated. The game
process uses about 24 GB of memory in the world.

## Status (2026-11-23)

**Not playable yet.** The title boots, runs its menus at its own 30 fps and
loads the world, where rendering fails.

| Stage | State |
|-------|-------|
| Boot, first-boot screens (accessibility, language, settings, legal) | works, 30 fps |
| Intro video with sound | works |
| Start screen, profile and saves | works |
| Loading into the world (about 50 s) | works |
| The world (first mission's intro) | reached and runs with indirect draws; its title card renders, the scene after it is still dark; 0.5-1.8 s per frame |

Tested on one machine: Windows 11, AMD RX 6700 XT.

> **Warning.** Entering the world on a real GPU can currently hang it, and
> Windows then resets the display driver (every monitor goes black for a
> moment). To look further, use a diagnostics build with
> `VICTORY_D3D12_WARP=1`, which renders on the software adapter.

What each part does today and what it still needs:

| Subsystem | Today | Needs |
|-----------|-------|-------|
| Loader | Runs the title's executable and its DLLs in our process; crash reports name the faulting code and are kept in `crashes.log` | – |
| Runtime services | One signed-in offline user, licences, packages, saves that survive a kill at any step, text entry dialog | Speech is silent; the dialog and relaunch are untested live |
| Network | Offline by design: the title cannot reach the network | – |
| Files and streaming | Archives and streamed data read, zlib and BCPack decoded in parallel | Loading is limited by writes into title memory |
| Input | XInput pads, keyboard, keyboard stand-in pad | Trigger rumble |
| Audio | Menu, video and loading sound; XMA voices and Opus streams decode | Reverb is silent; Opus results unverified against the title |
| GPU: menus | Draws, compute and presentation on host D3D12; only changed title memory is copied | Frame pacing drifts on one screen in release builds |
| GPU: world | Mesh pipelines, shader-written buffers, descriptor lifetimes, indirect execution (arguments clamped), pipelines made by worker threads and kept on disk | Most of the scene is still missing (untranslated shaders drop draws); frames take seconds; real queries, depth bounds, HDR and scaling are missing |
| Shaders | Console GPU code recompiled to DXIL at first use, cached on disk. Menus: all. World: mesh 8,191 of 8,506, vertex/pixel 10,617 of 12,346, compute 1,643 of 2,539 | 64-bit image atomics, merged geometry stages, the ray-tracing builder's shaders; some compute programs need a GPU with 64-lane waves |
| Textures | Console tiled layouts and detiling for the modes the title uses | Render-target metadata planes |
| Series S mode | Runs through the menus to the loading screen | The world, as above |

## Getting started

Requirements: Windows 10/11 x64, a CPU with AVX2, a Direct3D 12 GPU with
shader model 6.6 (and mesh shaders for the world), Rust (pinned in
`rust-toolchain.toml`).

Under Wine or Proton, indirect execution also needs DXC's `dxcompiler.dll` (with
its `dxil.dll`): Wine's `d3dcompiler_47` cannot compile the indirect-argument
clamp. It is looked for in `VICTORY_DXC` (the DLL or its folder), beside the
`dxil.dll` that shader validation finds, then the executable's folder and System32.

The title also needs several GiB of available Windows commit at startup.
Check **Committed** under **Task Manager → Performance → Memory**. If
`crashes.log` reports Windows error 1455, close other applications or increase
the Windows paging file size, then retry.

```powershell
cargo xtask install-hooks                # asset-policy check on every commit
copy victory.example.toml victory.toml   # then set your paths (outside the repo)
cargo test --workspace                   # needs no game data
cargo run -p victory -- config           # check your paths
```

The host SRV binding tests on Windows 10 may need the app-local
[DirectX Agility SDK](https://devblogs.microsoft.com/directx/directx12agility/).
Copy the x64 `D3D12Core.dll` and `d3d12SDKLayers.dll` from Microsoft.Direct3D.D3D12
1.620.2 into `target/debug/D3D12/` (or the equivalent Cargo target/profile
directory). Cargo detects them on the first build of `victory-gpu`. If the
crate has already been built, set `$env:VICTORY_TEST_AGILITY = '1'` before
running `cargo test -p victory-gpu --features diagnostics shader_usage_tests`.
Set `$env:VICTORY_TEST_AGILITY = '0'` to use the inbox runtime.

Run the title:

```powershell
cargo build --release -p victory
target\release\victory.exe run <game dir>\<the title's exe> --execute
```

The window caption shows the framerate. Saves, settings and `crashes.log`
are kept under the data folder (default `%LOCALAPPDATA%\victory\data`), derived
data under the cache folder.

Input: XInput pads. With none connected the keyboard is a pad while the
title's window has focus:

| Keys | Pad |
|------|-----|
| W A S D / I J K L | left / right stick |
| arrows | D-pad |
| Enter, Backspace, X, Y | A, B, X, Y |
| Q / E, Z / C | shoulders, triggers |
| F / H | stick clicks |
| Esc, Tab | Menu, View |

## Commands

```
victory inspect <exe|dll> [--imports] [--exports]
victory census [paths...] [--missing] [--strict]
victory lib-exports <import.lib> [--list]
victory shader-info [paths...] [--verbose] [--disasm]
victory translate [paths...] [--validate] [--pipelines] [--list] [--jobs N]
victory run <exe> [--execute] [--unimplemented trap|zero] [--strict]
            [--console series-x|series-s] [-- <title arguments>]
victory config
```

Logging: `VICTORY_LOG=victory_loader=trace,info` (a release build logs
info and above).

Host drivers that crash compiling a shader: with `VICTORY_PIPELINE_JOURNAL=1`
a pipeline whose creation ends the process is refused from the next run on
(its draws dropped, as for any refused pipeline) and listed in
`pipeline-journal\crashed.txt` under the cache folder. Each such pipeline
costs one crash; delete its line to try it again. Pipelines are created one
at a time while it is on, so a crash names only the pipeline that caused it.

## Debugging

A release build has no debugging aids. Build them in with

```powershell
cargo build --profile diag -p victory --features diagnostics   # target\diag\victory.exe
```

| Variable | Effect |
|----------|--------|
| `VICTORY_LOG=<target>=debug,...` | debug and trace output |
| `VICTORY_STACKS=20,60` | every thread's stack at those seconds |
| `VICTORY_WATCH=<rva>,...` | logs a title u32 when it changes |
| `VICTORY_KEYS=30:Enter,35:X` | presses keys at those seconds, focus or not |
| `VICTORY_CRASH_SAVE=<rva>:<len>,...` | saves title memory to the cache folder on a crash |
| `VICTORY_CAPTURE=<dir>` | saves presented frames (powers of two, or `VICTORY_CAPTURE_FRAMES=<list>`) |
| `VICTORY_DUMP_TARGETS=1` | with the two above, saves every render target of those frames |
| `VICTORY_D3D12_DEBUG=1` | host debug layer |
| `VICTORY_D3D12_GBV=1` | with the above, GPU-based validation (bad descriptor use reported, not faulted; slow) |
| `VICTORY_D3D12_DRED=1` | on a GPU failure, the allocation hit and the commands in flight |
| `VICTORY_SKIP_PIPELINES=<hash>,...` | omit draws and dispatches, including indirect ones, whose pipeline contains a listed 16-digit shader hash |
| `VICTORY_GPU_TIMING=1` | with the above, each list of a submission run and timed on its own; slow lists logged with their slowest commands |
| `VICTORY_D3D12_WARP=1` | host device on the software adapter (slow; cannot reset the driver) |
| `VICTORY_SHADER_LOOP_LIMIT=<n>` | translated shader loops stop after `n` iterations per lane instead of 2^20 (own shader cache entries); if a lower `n` makes world frames much faster, loops hitting the limit are what makes them slow |
| `VICTORY_AUDIO_CAPTURE=<file.wav>` | records what the title plays (our process only) |
| `VICTORY_XMA_DUMP=<dir>` | saves the first 32 XMA voices' context and input |

Every build appends crashes and host GPU failures to `crashes.log` in the
title's data folder.

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
