# Chostty

A GPU-accelerated terminal workspace manager for Linux, powered by Ghostty's rendering engine.

Chostty is a hard fork of [Limux](https://github.com/am-will/limux).

## Features

- **GPU-rendered terminals** via embedded Ghostty (OpenGL)
- **Workspaces** with folder-based naming, persistence across restarts, and sidebar management
- **Split panes** (horizontal/vertical) with keyboard navigation
- **Tabbed terminals** within each pane
- **Built-in browser** (WebKitGTK)
- **Editable keybindings** from Settings with JSON-backed persistence
- **Right-click context menu** with copy, paste, split, clear
- **Drag-and-drop** workspace reordering with favorites/pinning
- **Animated sidebar** collapse/expand

## Install

Chostty does not currently publish official GitHub releases. Today the supported paths are:

- build and run from source
- generate local packages with `./scripts/package.sh`

### Build from source

#### Prerequisites

- Rust toolchain (stable)
- Zig
- GTK4, libadwaita, WebKitGTK runtime and dev packages
- Initialized Ghostty submodule

```bash
# Install dev dependencies (Ubuntu/Debian)
sudo apt install libgtk-4-dev libadwaita-1-dev libwebkitgtk-6.0-dev pkg-config build-essential

# Initialize the Ghostty submodule and build the embedded library
git submodule update --init --recursive
(cd ghostty && zig build -Dapp-runtime=none -Doptimize=ReleaseFast)

# Build chostty
cargo build --release

# Run (point to libghostty.so location)
LD_LIBRARY_PATH=../ghostty/zig-out/lib:$LD_LIBRARY_PATH ./target/release/chostty
```

### Build local packages

```bash
./scripts/package.sh
```

This creates installable artifacts in `dist/`:

- `chostty_<version>_<arch>.deb`
- `chostty-<version>-linux-<arch>.tar.gz`
- `Chostty-<version>-<arch>.AppImage` when `appimagetool` is available

Examples:

**Debian/Ubuntu (.deb)**
```bash
sudo dpkg -i ./dist/chostty_<version>_<arch>.deb
```

**AppImage**
```bash
chmod +x ./dist/Chostty-<version>-<arch>.AppImage
./dist/Chostty-<version>-<arch>.AppImage
```

**Tarball**
```bash
tar xzf ./dist/chostty-<version>-linux-<arch>.tar.gz
cd chostty-<version>-linux-<arch>
sudo ./install.sh
```

To uninstall:
```bash
# deb
sudo apt remove chostty

# tarball
sudo ./install.sh --uninstall
```

### System dependencies

```bash
# Ubuntu/Debian
sudo apt install libgtk-4-1 libadwaita-1-0 libwebkitgtk-6.0-4
```

Chostty now requires `libadwaita >= 1.5`, which is available in Ubuntu 24.04+ and Debian 13+.

## Refresh Ghostty From Upstream

To replay Chostty's minimal embedded-Linux Ghostty patch queue onto the latest
upstream Ghostty `main`:

```bash
./scripts/update_ghostty.sh
```

To replay the patch queue onto a specific upstream ref:

```bash
./scripts/update_ghostty.sh --ref <commit-or-tag>
```

This script fetches `ghostty-org/ghostty`, resets the local `ghostty`
submodule checkout to a clean upstream base, and applies the patches from
`patches/ghostty-upstream-rebase/`.
It reuses your Chostty repo `git config user.name` and `user.email` for the
temporary `git am` replay unless you override them with script flags.

See [`docs/ghostty-upstream-rebase.md`](docs/ghostty-upstream-rebase.md) for
the patch queue and rebase notes.

## Development

Run the canonical local quality gate before committing:

```bash
./scripts/check.sh
```

Repository maintainability rules live in [`docs/maintainability.md`](docs/maintainability.md).

## Keyboard shortcuts

Most default shortcuts use `Ctrl`. Fullscreen defaults to `F11`. Custom remaps may also use `Cmd`, which Chostty maps to either the Linux `Meta` or `Super` modifier. `Opt` maps to `Alt`.

You can edit shortcuts from Settings > Keybindings. Remaps are stored in `~/.config/chostty/shortcuts.json`.

### App

| Shortcut | Action |
|---|---|
| `Ctrl+Shift+N` | New workspace |
| `Ctrl+Alt+R` | Rename active workspace |
| `Ctrl+Shift+P` | Open workspace by path |
| `Ctrl+Shift+W` | Close workspace |
| `Ctrl+Q` | Quit Chostty |
| `Ctrl+Alt+N` | Open a new Chostty instance |
| `Ctrl+,` | Open settings |
| `Ctrl+M` | Toggle sidebar |
| `Ctrl+Shift+M` | Toggle top bar |
| `F11` | Toggle fullscreen |
| `Ctrl+PageDown` | Next workspace |
| `Ctrl+PageUp` | Previous workspace |
| `Ctrl+Shift+PageUp` | Move active workspace up |
| `Ctrl+Shift+PageDown` | Move active workspace down |
| `Ctrl+1-8` | Switch to workspace by number |
| `Ctrl+9` | Switch to the last workspace in the list |

### Browser

| Shortcut | Action |
|---|---|
| `Ctrl+Shift+L` | Open the focused browser page in a new split |
| `Ctrl+L` | Focus browser address bar |
| `Ctrl+[` | Browser back |
| `Ctrl+]` | Browser forward |
| `Ctrl+R` | Browser reload |
| `Ctrl+Alt+I` | Open Web Inspector |
| `Ctrl+Alt+C` | Open Web Inspector (console-only targeting is not exposed by WebKitGTK) |

### Find

| Shortcut | Action |
|---|---|
| `Ctrl+F` | Open find on the focused terminal or browser |
| `Ctrl+G` | Find next |
| `Ctrl+Shift+G` | Find previous |
| `Ctrl+Shift+F` | Hide find |
| `Ctrl+E` | Use selection for find |

### Terminal

| Shortcut | Action |
|---|---|
| `Ctrl+K` | Clear scrollback |
| `Ctrl+Shift+C` | Copy selection |
| `Ctrl+Shift+V` | Paste |
| `Ctrl++` | Increase font size |
| `Ctrl+-` | Decrease font size |
| `Ctrl+Shift+0` | Reset font size |

### Workspace And Pane

| Shortcut | Action |
|---|---|
| `Ctrl+Shift+Left/Right` | Cycle tabs in focused pane |
| `Ctrl+Shift+D` | Split down |
| `Ctrl+Shift+T` | New terminal in focused pane |
| `Ctrl+D` | Split right |
| `Ctrl+W` | Close the active tab in the focused pane |
| `Ctrl+T` | New terminal tab |
| `Ctrl+Arrow` | Focus pane in direction |

## Architecture

```
rust/
  chostty-host-linux/    # GTK4/Adwaita UI (window, sidebar, panes, tabs)
  chostty-ghostty-sys/   # FFI bindings to libghostty
  chostty-core/          # Command dispatcher and state engine
  chostty-protocol/      # Socket wire format types
  chostty-control/       # Unix socket server
  chostty-cli/           # CLI client
```

The terminal rendering is handled entirely by Ghostty's embedded library (`libghostty.so`), which provides GPU-accelerated OpenGL rendering. The UI layer is native GTK4 with libadwaita.

## License

MIT
