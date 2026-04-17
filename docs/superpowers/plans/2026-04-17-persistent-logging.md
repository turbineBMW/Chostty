# Persistent File Logging Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Capture Rust panics, raw C-level stderr, and structured `tracing` breadcrumbs to persistent files under `$XDG_STATE_HOME/chostty/` so that the ~daily crash leaves an investigable trail.

**Architecture:** New `logging` module in `chostty-host-linux` called once at the top of `main()`. Uses `tracing` + `tracing-subscriber` + `tracing-appender` for structured events (blocking writer, daily rotation, 7 files retained) and `libc::dup2` to redirect fd 2 to a session-scoped `chostty.stderr.log` (truncated each launch). Panic hook emits a `tracing::error!` with backtrace before chaining to the previous hook. Host-linux crate only; no workspace-level changes.

**Tech Stack:** Rust, `tracing = "0.1"`, `tracing-subscriber = "0.3"` (env-filter + fmt), `tracing-appender = "0.2"`, `libc = "0.2"`. Existing `dirs = "6"` for `state_dir()`.

**Spec:** `docs/superpowers/specs/2026-04-17-persistent-logging-design.md`

---

## File structure

**Create:**
- `rust/chostty-host-linux/src/logging.rs` — the module (log_dir, redirect_stderr, init_tracing, install_panic_hook, init).

**Modify:**
- `rust/chostty-host-linux/Cargo.toml` — add tracing + libc deps.
- `rust/chostty-host-linux/src/main.rs` — `mod logging;` and call `logging::init()` at top of `main()`.
- `rust/chostty-host-linux/src/window.rs` — instrumentation at workspace/pane/tab/paste sites (Task 6–7).
- `rust/chostty-host-linux/src/terminal.rs` — instrumentation at terminal spawn/exit sites (Task 7).
- `README.md` — document log file locations and `RUST_LOG` override (Task 8).

**Note on ordering:** Tasks 1–5 land the core crash-capture capability (panics + stderr + minimal startup/shutdown tracing). The next crash after that lands logs. Tasks 6–7 add the remaining event breadcrumbs. Task 8 documents it.

---

## Task 1: Add dependencies and module skeleton with `log_dir()` + unit tests

**Files:**
- Modify: `rust/chostty-host-linux/Cargo.toml`
- Create: `rust/chostty-host-linux/src/logging.rs`

- [ ] **Step 1.1: Add dependencies to `rust/chostty-host-linux/Cargo.toml`**

After the `shell-quote` line, add:

```toml
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }
tracing-appender = "0.2"
libc = "0.2"
```

The `[dependencies]` block should now contain (additions only):

```toml
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }
tracing-appender = "0.2"
libc = "0.2"
```

- [ ] **Step 1.2: Verify the dependencies resolve**

Run: `cargo check -p chostty-host-linux`
Expected: Compiles cleanly (no code uses the new deps yet, so this just confirms versions resolve).

- [ ] **Step 1.3: Create `rust/chostty-host-linux/src/logging.rs` with `log_dir()` and tests**

```rust
//! Persistent file logging for chostty-host-linux.
//!
//! See docs/superpowers/specs/2026-04-17-persistent-logging-design.md.

use std::path::PathBuf;

/// Resolve the directory that holds chostty's log files.
///
/// Order:
/// 1. `$XDG_STATE_HOME/chostty` if `XDG_STATE_HOME` is set and absolute.
/// 2. `~/.local/state/chostty` via `dirs::state_dir()`.
/// 3. `~/.local/state/chostty` hand-built from `$HOME`.
/// 4. `/tmp/chostty` last resort.
fn log_dir() -> PathBuf {
    if let Some(raw) = std::env::var_os("XDG_STATE_HOME") {
        let p = PathBuf::from(raw);
        if p.is_absolute() {
            return p.join("chostty");
        }
    }
    if let Some(p) = dirs::state_dir() {
        return p.join("chostty");
    }
    if let Some(home) = dirs::home_dir() {
        return home.join(".local").join("state").join("chostty");
    }
    PathBuf::from("/tmp/chostty")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    // Serialize tests that mutate process-wide env vars.
    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn log_dir_honors_xdg_state_home_when_absolute() {
        let _g = env_lock();
        let prev = std::env::var_os("XDG_STATE_HOME");
        std::env::set_var("XDG_STATE_HOME", "/var/tmp/chostty-test-state");

        let got = log_dir();
        assert_eq!(got, Path::new("/var/tmp/chostty-test-state/chostty"));

        match prev {
            Some(v) => std::env::set_var("XDG_STATE_HOME", v),
            None => std::env::remove_var("XDG_STATE_HOME"),
        }
    }

    #[test]
    fn log_dir_ignores_relative_xdg_state_home() {
        let _g = env_lock();
        let prev = std::env::var_os("XDG_STATE_HOME");
        std::env::set_var("XDG_STATE_HOME", "relative/path");

        let got = log_dir();
        // Must NOT be the relative value joined with "chostty".
        assert!(
            got != Path::new("relative/path/chostty"),
            "relative XDG_STATE_HOME must not be used: got {:?}",
            got
        );
        // Must be absolute (fell back to state_dir() or HOME).
        assert!(
            got.is_absolute(),
            "fallback must produce an absolute path: got {:?}",
            got
        );
        assert!(
            got.ends_with("chostty"),
            "fallback must still end in 'chostty': got {:?}",
            got
        );

        match prev {
            Some(v) => std::env::set_var("XDG_STATE_HOME", v),
            None => std::env::remove_var("XDG_STATE_HOME"),
        }
    }

    #[test]
    fn log_dir_falls_back_when_xdg_state_home_unset() {
        let _g = env_lock();
        let prev = std::env::var_os("XDG_STATE_HOME");
        std::env::remove_var("XDG_STATE_HOME");

        let got = log_dir();
        assert!(
            got.ends_with("chostty"),
            "fallback path must end in 'chostty': got {:?}",
            got
        );
        assert!(got.is_absolute(), "fallback path must be absolute: got {:?}", got);

        match prev {
            Some(v) => std::env::set_var("XDG_STATE_HOME", v),
            None => std::env::remove_var("XDG_STATE_HOME"),
        }
    }
}
```

- [ ] **Step 1.4: Register the module in `rust/chostty-host-linux/src/main.rs`**

Add to the top of `main.rs` with the other `mod` declarations (after `mod layout_state;`):

```rust
mod logging;
```

The final `mod` block (lines 1–12 plus the addition) should contain `mod logging;` before `mod notification_sound;` alphabetically, or anywhere in the list — order doesn't matter. Just add one line.

- [ ] **Step 1.5: Run the tests**

Run: `cargo test -p chostty-host-linux --lib logging::tests`
Expected: 3 passed.

- [ ] **Step 1.6: Commit**

```bash
git add rust/chostty-host-linux/Cargo.toml rust/chostty-host-linux/src/logging.rs rust/chostty-host-linux/src/main.rs
git commit -m "Add logging module skeleton with log_dir resolver

Resolves $XDG_STATE_HOME/chostty with fallback to ~/.local/state/chostty.
No wiring into main() yet — that lands after the remaining functions
(stderr redirect, tracing subscriber, panic hook) are in place."
```

---

## Task 2: Add `redirect_stderr()` that truncates the file and `dup2`s fd 2

**Files:**
- Modify: `rust/chostty-host-linux/src/logging.rs`

- [ ] **Step 2.1: Add `redirect_stderr` below `log_dir` in `logging.rs`**

Add these imports at the top of the file (below the existing `use`):

```rust
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::fd::AsRawFd;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
```

Then add the function after `log_dir`:

```rust
/// Truncate-open `chostty.stderr.log` in `dir`, write a session banner, and
/// `dup2` the file's fd onto fd 2 so that any C-level stderr output (GTK,
/// GLib, libghostty) lands in the file for the lifetime of this process.
///
/// The `File` is deliberately leaked after `dup2` — closing it would not
/// affect fd 2 (dup2 gave it an independent slot in the kernel fd table),
/// but leaking makes the ownership intent explicit and avoids ever running
/// any `Drop` on it.
fn redirect_stderr(dir: &Path) -> io::Result<()> {
    let path = dir.join("chostty.stderr.log");
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&path)?;

    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "unknown".to_string());
    let banner = format!(
        "=== chostty v{} pid={} started epoch={} ===\n",
        crate::VERSION,
        std::process::id(),
        secs,
    );
    file.write_all(banner.as_bytes())?;
    file.flush()?;

    let rc = unsafe { libc::dup2(file.as_raw_fd(), libc::STDERR_FILENO) };
    if rc < 0 {
        return Err(io::Error::last_os_error());
    }

    std::mem::forget(file);
    Ok(())
}
```

- [ ] **Step 2.2: Verify it still compiles**

Run: `cargo check -p chostty-host-linux`
Expected: Compiles with no warnings about the new function (it's unused — compiler will warn; ignore for now, it gets used in Task 5).

If the unused-function warning fails CI, silence it locally by marking the function `#[allow(dead_code)]` — remove that attribute in Task 5 when `init()` calls it.

```rust
#[allow(dead_code)]
fn redirect_stderr(dir: &Path) -> io::Result<()> { ... }
```

- [ ] **Step 2.3: Commit**

```bash
git add rust/chostty-host-linux/src/logging.rs
git commit -m "Add redirect_stderr() for capturing GTK/GLib/libghostty stderr

Opens chostty.stderr.log truncating, writes a session banner, and
dup2's the file fd onto fd 2. The File is leaked so the backing fd
persists for the process lifetime."
```

---

## Task 3: Add `init_tracing()` with rolling daily appender and `EnvFilter`

**Files:**
- Modify: `rust/chostty-host-linux/src/logging.rs`

- [ ] **Step 3.1: Add `init_tracing` to `logging.rs`**

Add these imports to the top of the file:

```rust
use tracing_appender::rolling::{Builder, Rotation};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};
```

Then add after `redirect_stderr`:

```rust
/// Install the global `tracing` subscriber with a daily-rotating file
/// appender at `dir/chostty.log.*`. Uses the appender as a **blocking
/// writer** (no background worker, no non-blocking buffer) so that every
/// event is on disk before the call returns — critical when the next line
/// of C code might segfault.
///
/// Respects `RUST_LOG`; defaults to `info` when unset.
///
/// Returns an error only if the rolling appender fails to build
/// (e.g. directory not writable); in that case the caller should fall back
/// to an stderr-only subscriber.
fn init_tracing(dir: &Path) -> io::Result<()> {
    let appender = Builder::new()
        .rotation(Rotation::DAILY)
        .max_log_files(7)
        .filename_prefix("chostty")
        .filename_suffix("log")
        .build(dir)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));

    // ANSI off: the file should contain plain text, not escape codes.
    let layer = fmt::layer()
        .with_writer(appender)
        .with_ansi(false)
        .with_target(true)
        .with_thread_ids(true)
        .with_line_number(true);

    tracing_subscriber::registry()
        .with(filter)
        .with(layer)
        .try_init()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

    Ok(())
}

/// Fallback when the rolling appender can't be built: install a plain
/// stderr subscriber so `tracing!` calls are at least visible if the app
/// is launched from a terminal.
fn init_tracing_stderr_fallback() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));
    let layer = fmt::layer().with_ansi(false);
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(layer)
        .try_init();
}
```

- [ ] **Step 3.2: Mark as unused (temporary)**

Add `#[allow(dead_code)]` above both new functions — they get called from `init()` in Task 5.

- [ ] **Step 3.3: Verify compilation**

Run: `cargo check -p chostty-host-linux`
Expected: Compiles. Any errors about `max_log_files` not existing mean `tracing-appender` needs to be `>= 0.2.3`; bump the version constraint if so.

- [ ] **Step 3.4: Commit**

```bash
git add rust/chostty-host-linux/src/logging.rs
git commit -m "Add init_tracing() with daily rolling file appender

Files: chostty.log.YYYY-MM-DD, 7 retained. Blocking writer so no
events are lost on segfault. EnvFilter defaults to info, respects
RUST_LOG. Separate stderr fallback for directory-write failures."
```

---

## Task 4: Add `install_panic_hook()` that chains in front of the default

**Files:**
- Modify: `rust/chostty-host-linux/src/logging.rs`

- [ ] **Step 4.1: Add `install_panic_hook` to `logging.rs`**

Add after `init_tracing_stderr_fallback`:

```rust
/// Install a panic hook that logs the panic via `tracing::error!` (so it
/// lands in `chostty.log`) and then delegates to the previously installed
/// hook — whose default behavior prints the panic to stderr, which after
/// Task 5's redirect means it lands in `chostty.stderr.log` too.
fn install_panic_hook() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let payload = info.payload();
        let message = payload
            .downcast_ref::<&'static str>()
            .copied()
            .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
            .unwrap_or("<non-string panic payload>");
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "<unknown>".to_string());
        let backtrace = std::backtrace::Backtrace::force_capture();

        tracing::error!(
            panic.message = %message,
            panic.location = %location,
            panic.backtrace = %backtrace,
            "rust panic"
        );

        prev(info);
    }));
}
```

Also add `#[allow(dead_code)]` above it (removed in Task 5).

- [ ] **Step 4.2: Verify compilation**

Run: `cargo check -p chostty-host-linux`
Expected: Compiles.

- [ ] **Step 4.3: Commit**

```bash
git add rust/chostty-host-linux/src/logging.rs
git commit -m "Add install_panic_hook() that emits tracing::error! with backtrace

Chains in front of the previous hook so the default stderr-print
behavior still runs (which, after the redirect, writes to
chostty.stderr.log too)."
```

---

## Task 5: Add `init()` orchestrator and wire it into `main()`

**Files:**
- Modify: `rust/chostty-host-linux/src/logging.rs`
- Modify: `rust/chostty-host-linux/src/main.rs`

- [ ] **Step 5.1: Add `init()` and remove `#[allow(dead_code)]` attributes**

Remove all `#[allow(dead_code)]` attributes added in Tasks 2–4. Add this public function at the top of the function list (right below the module-level docs, above `log_dir`):

```rust
/// One-shot initializer for chostty's logging. Call once, first thing in
/// `main()`. Fail-soft: every step is independent, and any failure falls
/// back to the most-useful remaining behavior (stderr subscriber, or
/// nothing) rather than aborting the program.
pub fn init() {
    let dir = log_dir();

    if let Err(err) = fs::create_dir_all(&dir) {
        // Write directly to the real stderr — `init_tracing` below may
        // still succeed if the directory was created by another racing
        // process, but we log the failure regardless.
        eprintln!(
            "chostty: failed to create log directory {}: {err}",
            dir.display()
        );
        init_tracing_stderr_fallback();
        install_panic_hook();
        return;
    }

    // Redirect first, so any output from init_tracing (unlikely) or any
    // C-level library called afterwards lands in the stderr file.
    if let Err(err) = redirect_stderr(&dir) {
        eprintln!(
            "chostty: failed to redirect stderr to {}: {err}",
            dir.join("chostty.stderr.log").display()
        );
        // Keep going — tracing-to-file still useful even without stderr capture.
    }

    if let Err(err) = init_tracing(&dir) {
        // Avoid `tracing!` calls here — the subscriber isn't installed.
        // Write to (redirected) stderr so the message lands in the stderr file.
        eprintln!(
            "chostty: failed to install rolling tracing appender in {}: {err}",
            dir.display()
        );
        init_tracing_stderr_fallback();
    }

    install_panic_hook();

    tracing::info!(
        event = "startup",
        version = crate::VERSION,
        pid = std::process::id(),
        log_dir = %dir.display(),
        "chostty started"
    );
}
```

- [ ] **Step 5.2: Call `logging::init()` at the top of `main()`**

In `rust/chostty-host-linux/src/main.rs`, modify `main()` (starts at line 135):

Replace:

```rust
fn main() {
    // Handle --version flag
    if std::env::args().any(|a| a == "--version" || a == "-v") {
        println!("Chostty {VERSION}");
        return;
    }
```

With:

```rust
fn main() {
    // Handle --version flag (before logging init so `--version` stays a
    // side-effect-free, instant command).
    if std::env::args().any(|a| a == "--version" || a == "-v") {
        println!("Chostty {VERSION}");
        return;
    }

    // Install persistent file logging before anything else. Captures
    // Rust panics, redirects C-level stderr into a file, and starts
    // structured event logging.
    logging::init();
```

- [ ] **Step 5.3: Verify compilation**

Run: `cargo check -p chostty-host-linux`
Expected: Clean compile.

- [ ] **Step 5.4: Run existing tests to confirm nothing broke**

Run: `cargo test -p chostty-host-linux`
Expected: All tests pass (3 new ones in `logging::tests`, plus the existing `main::tests`).

- [ ] **Step 5.5: Smoke test — run the app and verify files appear**

Run:

```bash
(cd ghostty && zig build -Dapp-runtime=none -Doptimize=Debug) && \
  cargo build -p chostty-host-linux && \
  LD_LIBRARY_PATH=./ghostty/zig-out/lib:$LD_LIBRARY_PATH \
  ./target/debug/chostty &
CHOSTTY_PID=$!
sleep 3
kill $CHOSTTY_PID
wait $CHOSTTY_PID 2>/dev/null
ls -la ~/.local/state/chostty/
```

Expected: `~/.local/state/chostty/` contains:
- A file named `chostty.log.YYYY-MM-DD` (today's date) with at least one line containing `event=startup`, `version=0.1.12`, and `chostty started`.
- A file named `chostty.stderr.log` starting with a `=== chostty v0.1.12 pid=N started epoch=... ===` banner.

Verify:

```bash
grep "chostty started" ~/.local/state/chostty/chostty.log.*
head -n 1 ~/.local/state/chostty/chostty.stderr.log
```

If the build steps fail because the Ghostty submodule isn't built, run the build command from README first.

- [ ] **Step 5.6: Commit**

```bash
git add rust/chostty-host-linux/src/logging.rs rust/chostty-host-linux/src/main.rs
git commit -m "Wire logging::init() into main() and emit startup event

Initializes persistent file logging before GTK/Ghostty setup so any
warnings from those subsystems are captured. Fail-soft at every
step — log dir failure, dup2 failure, or appender failure all
degrade gracefully without aborting the app."
```

---

## Task 6: Instrument startup/shutdown and paste sites (highest-signal breadcrumbs)

**Files:**
- Modify: `rust/chostty-host-linux/src/main.rs`
- Modify: `rust/chostty-host-linux/src/window.rs`
- Modify: `rust/chostty-host-linux/src/terminal.rs`

This task adds the first wave of breadcrumbs — the ones most likely to help diagnose the current daily crash. Paste was the subject of the most recent bug-fix (`150440d Fixed issues with Limux pasting to ghostty`), so any paste-adjacent event belongs here.

- [ ] **Step 6.1: Log shutdown in `main.rs`**

In `main.rs`, after `app.run();` in `main()`, add:

```rust
    app.run();

    tracing::info!(event = "shutdown", "chostty exiting cleanly");
}
```

- [ ] **Step 6.2: Instrument `ShortcutCommand::TerminalPaste` in `window.rs`**

Find the line at `rust/chostty-host-linux/src/window.rs:4296` (`ShortcutCommand::TerminalPaste => target.perform_binding_action("paste_from_clipboard"),`). Replace with:

```rust
ShortcutCommand::TerminalPaste => {
    tracing::info!(
        event = "paste",
        source = "shortcut",
        "paste requested via shortcut"
    );
    target.perform_binding_action("paste_from_clipboard")
}
```

(If the line has moved by the time you edit it, grep for `ShortcutCommand::TerminalPaste` to locate it.)

- [ ] **Step 6.3: Instrument the context-menu paste in `terminal.rs`**

Find the line at `rust/chostty-host-linux/src/terminal.rs:1643` (`"Paste" => surface_action(surface, "paste_from_clipboard"),`). Replace with:

```rust
"Paste" => {
    tracing::info!(
        event = "paste",
        source = "context_menu",
        "paste requested via context menu"
    );
    surface_action(surface, "paste_from_clipboard")
}
```

- [ ] **Step 6.4: Verify compilation**

Run: `cargo check -p chostty-host-linux`
Expected: Clean compile.

- [ ] **Step 6.5: Manual verification**

Build and run the app. Paste into a terminal via `Ctrl+Shift+V` once, then via right-click → Paste once, then quit.

Run:

```bash
grep '"event":"paste"\|event=paste' ~/.local/state/chostty/chostty.log.*
grep 'shutdown' ~/.local/state/chostty/chostty.log.*
```

Expected: Two paste events (one `source=shortcut`, one `source=context_menu`) and one shutdown event.

- [ ] **Step 6.6: Commit**

```bash
git add rust/chostty-host-linux/src/main.rs rust/chostty-host-linux/src/window.rs rust/chostty-host-linux/src/terminal.rs
git commit -m "Log paste events and clean shutdown

First wave of breadcrumbs: paste source (shortcut vs context menu)
and clean shutdown event. These are the highest-signal events for
diagnosing the recent paste-related crash class."
```

---

## Task 7: Instrument workspace / pane / tab / terminal-spawn / browser events

**Files:**
- Modify: `rust/chostty-host-linux/src/window.rs`
- Modify: `rust/chostty-host-linux/src/terminal.rs`

This task adds the remaining INFO-level breadcrumbs listed in the spec. Use the Grep tool to locate each site; the plan gives the search pattern and the instrumentation template, because chasing exact line numbers in a file that is actively being edited is brittle.

- [ ] **Step 7.1: Workspace events in `window.rs`**

Grep for the function names below in `rust/chostty-host-linux/src/window.rs` and add a `tracing::info!` at the top of each function body:

- Functions containing "workspace" + "create" / "new" — `event = "workspace_create"`, fields: workspace name if available.
- Functions containing "workspace" + "close" / "remove" — `event = "workspace_close"`.
- Functions containing "workspace" + "rename" — `event = "workspace_rename"`, fields: old name, new name.
- Functions containing "workspace" + "activate" / "switch" / "select" — `event = "workspace_switch"`, fields: index or name.
- Functions containing "workspace" + "move" / "reorder" — `event = "workspace_reorder"`, fields: from index, to index.

Example template:

```rust
tracing::info!(
    event = "workspace_switch",
    index = new_index,
    "workspace switched"
);
```

For each site, prefer logging **indices or names**, never file paths that include the user's home prefix in plain form if avoidable. (Paths are fine where they're already in the code — just don't construct new ones.)

- [ ] **Step 7.2: Pane split / close in `window.rs` (or `pane.rs` / `split_tree.rs`)**

Grep for `split_` / `split_down` / `split_right` / `close_pane` / `remove_pane`. Add:

```rust
tracing::info!(event = "pane_split", direction = "down", "pane split");
// or
tracing::info!(event = "pane_close", "pane closed");
```

- [ ] **Step 7.3: Tab events in `terminal.rs` (or `window.rs`, whichever holds them)**

Grep for tab-creation/close/reorder/switch sites. Typical names: `new_tab`, `close_tab`, `move_tab`, `reorder_tab`, `activate_tab`, `select_tab`. Add:

```rust
tracing::info!(event = "tab_new", "tab created");
tracing::info!(event = "tab_close", index = i, "tab closed");
tracing::info!(event = "tab_reorder", from = i, to = j, "tab reordered");
tracing::info!(event = "tab_switch", index = i, "tab switched");
```

- [ ] **Step 7.4: Terminal spawn / exit in `terminal.rs`**

Grep for the function that spawns a child process (shell) for a new terminal; typical names: `spawn_`, `new_surface`, `create_surface`, or the function that reads the shell path. Add at spawn site:

```rust
tracing::info!(
    event = "terminal_spawn",
    shell = %shell_path.display(),
    cwd = %cwd.display(),
    "terminal spawned"
);
```

For exit: find the callback that fires when a child exits. If the codebase doesn't have one yet, skip the exit event and note it in the commit message.

```rust
tracing::info!(
    event = "terminal_exit",
    exit_code = code,
    "terminal exited"
);
```

- [ ] **Step 7.5: Browser navigate**

Grep for `load_uri` or `navigate` in `window.rs` (browser ops). At the function that actually calls WebKit's `load_uri`, add:

```rust
let host = url::Url::parse(&uri).ok().and_then(|u| u.host_str().map(str::to_owned));
tracing::info!(
    event = "browser_navigate",
    host = host.as_deref().unwrap_or("<unparseable>"),
    "browser navigate"
);
```

If `url` is not already a workspace dep, use a simpler logging strategy that extracts the host with basic string operations, or log only the scheme + length (`scheme`, `url_len`). **Never log the full URL** — query strings may contain tokens.

Cheap host extraction without a new dep:

```rust
fn log_host_only(uri: &str) -> String {
    // Extract scheme://host[:port] only, dropping path and query.
    let after_scheme = uri.splitn(2, "://").nth(1).unwrap_or(uri);
    let host_and_port = after_scheme.splitn(2, '/').next().unwrap_or(after_scheme);
    host_and_port.splitn(2, '?').next().unwrap_or(host_and_port).to_string()
}
```

Place this helper in `window.rs` near the browser code. Prefer this over adding the `url` crate for one call site.

- [ ] **Step 7.6: Verify compilation**

Run: `cargo check -p chostty-host-linux`
Expected: Clean compile. Fix any import issues (tracing macros are free functions — no imports needed for `tracing::info!`).

- [ ] **Step 7.7: Run tests**

Run: `cargo test -p chostty-host-linux`
Expected: All pass.

- [ ] **Step 7.8: Manual smoke**

Launch the app. Do: create a workspace, switch workspaces, split a pane, open 2 tabs, reorder them, close one, open a browser tab and navigate to one URL, quit.

Run:

```bash
grep -oE 'event="?[a-z_]+' ~/.local/state/chostty/chostty.log.* | sort -u
```

Expected: Seeing the events you actually exercised: `startup`, `paste` (if you pasted), `workspace_create`, `workspace_switch`, `pane_split`, `tab_new`, `tab_reorder`, `tab_close`, `browser_navigate`, `shutdown`.

- [ ] **Step 7.9: Commit**

```bash
git add rust/chostty-host-linux/src/window.rs rust/chostty-host-linux/src/terminal.rs
git commit -m "Instrument workspace, pane, tab, terminal, and browser events

Adds the remaining INFO-level breadcrumbs from the spec. Browser
events log host only, never full URL, to avoid leaking tokens."
```

---

## Task 8: Document log locations in the README

**Files:**
- Modify: `README.md`

- [ ] **Step 8.1: Add a "Logs" section to the README**

Add a new section between "Keyboard shortcuts" and "Architecture" (so it appears before the nuts-and-bolts architecture overview):

```markdown
## Logs

Chostty writes two log files under `$XDG_STATE_HOME/chostty/` (typically
`~/.local/state/chostty/`):

- `chostty.log.YYYY-MM-DD` — structured events (startup, paste, workspace /
  pane / tab operations, terminal spawn/exit, browser navigate, Rust
  panics with backtrace). Rotated daily; 7 files retained.
- `chostty.stderr.log` — raw stderr captured from GTK, GLib, WebKit, and
  libghostty. **Truncated on each launch**, so this file always
  corresponds to the current process.

### Adjusting verbosity

Set `RUST_LOG` before launching. Examples:

```bash
RUST_LOG=chostty=debug chostty         # include DEBUG events
RUST_LOG=warn chostty                  # WARN and above only
```

### For crash diagnosis

If Chostty crashes, capture both files plus the core dump:

```bash
cp ~/.local/state/chostty/chostty.log.$(date +%F) /tmp/
cp ~/.local/state/chostty/chostty.stderr.log /tmp/
coredumpctl info chostty | head -n 100 > /tmp/coredump-info.txt
```

`chostty.stderr.log` will contain any GLib critical warnings that
typically precede these crashes; `coredumpctl` has the real stack trace
with symbols.
```

- [ ] **Step 8.2: Commit**

```bash
git add README.md
git commit -m "Document persistent log files and crash-diagnosis workflow

Explains $XDG_STATE_HOME/chostty/ layout, RUST_LOG overrides, and
the recommended capture steps when a crash happens."
```

---

## Done

At this point the daily crash will leave:
- A structured event trail up to the crash line in `chostty.log.<date>`.
- Any C-level warnings from GTK/GLib/libghostty in `chostty.stderr.log`.
- A Rust panic with backtrace (for panics) in both files.
- A core dump via `systemd-coredump` for segfaults (pre-existing mechanism, unchanged by this plan).

Next crash can be diagnosed by:

```bash
tail -n 200 ~/.local/state/chostty/chostty.log.$(date +%F)
cat ~/.local/state/chostty/chostty.stderr.log
coredumpctl gdb chostty       # if systemd caught a segfault
```
