# Ghostty Upstream Rebase Notes

This repo currently depends on Ghostty embedded-Linux behavior that does not
come from upstream `ghostty-org/ghostty` out of the box.

The full `am-will/ghostty` fork is not required for Chostty. The Chostty host
only depends on a small Linux-focused patch stack:

1. `Add Linux platform support to embedded apprt`
2. `Add display lifecycle API and fix drawFrame sync for embedded Linux`
3. `embedded: gate paste on text clipboard availability`

The cmux theme-picker commits from the fork were intentionally excluded because
Chostty does not call into that Ghostty functionality.

## Tested Upstream Base

- Upstream repository: `https://github.com/ghostty-org/ghostty.git`
- Upstream commit fetched on 2026-04-09: `28972454c`

The minimal patch stack was successfully replayed onto that upstream commit as:

- `e63b250c1` `Add Linux platform support to embedded apprt`
- `ea41ced2c` `Add display lifecycle API and fix drawFrame sync for embedded Linux`
- `11bf8cf3d` `embedded: gate paste on text clipboard availability`

The first two commits cherry-picked cleanly. The third required a small manual
rebase in these files because upstream's embedded clipboard API had drifted:

- `src/apprt/embedded.zig`
- `macos/Sources/Ghostty/Ghostty.App.swift`

## Patch Queue

Replay these patches from this repo:

- `patches/ghostty-upstream-rebase/0001-Add-Linux-platform-support-to-embedded-apprt.patch`
- `patches/ghostty-upstream-rebase/0002-Add-display-lifecycle-API-and-fix-drawFrame-sync-for.patch`
- `patches/ghostty-upstream-rebase/0003-embedded-gate-paste-on-text-clipboard-availability.patch`

## Suggested Update Flow

The canonical repo workflow is:

```bash
./scripts/update_ghostty.sh
```

To target a specific upstream Ghostty commit, branch, or tag:

```bash
./scripts/update_ghostty.sh --ref <commit-or-tag>
```

The script fetches upstream Ghostty into the `ghostty` submodule, resets a
local sync branch, and replays the Chostty patch queue with `git am`.
By default it inherits `user.name` and `user.email` from the parent Chostty
repo if the `ghostty` submodule does not have its own Git identity configured.

## Manual Fallback

If you need to replay the queue by hand, the equivalent flow is:

```bash
git clone https://github.com/ghostty-org/ghostty.git
cd ghostty
git checkout <new-upstream-commit>
git am /path/to/chostty/patches/ghostty-upstream-rebase/*.patch
```

If `git am` fails on a future upstream update, start by checking:

- embedded platform tags and runtime callback signatures in `include/ghostty.h`
- clipboard flow in `src/apprt/embedded.zig`
- embedded OpenGL lifecycle handling in `src/renderer/OpenGL.zig`
- macOS embedded callback glue in `macos/Sources/Ghostty/Ghostty.App.swift`

## Chostty Integration Points

These Chostty files are the direct consumers of the Linux embedded Ghostty
patches:

- `rust/chostty-ghostty-sys/src/lib.rs`
- `rust/chostty-host-linux/src/terminal.rs`

In particular, Chostty uses:

- `GHOSTTY_PLATFORM_LINUX`
- `ghostty_surface_display_realized`
- `ghostty_surface_display_unrealized`
- the embedded clipboard text-availability behavior for paste handling
