# Persistent File Logging — Design

Status: design approved, pending implementation plan
Scope: `rust/chostty-host-linux` only
Motivation: Chostty crashes approximately once per day with no persistent
trace. The app currently installs no tracing subscriber, no panic hook, and
no stderr redirect, so any GTK/GLib/libghostty warnings that precede a crash
are lost once the terminal or desktop session ends.

## Goals

- Capture every Rust panic (with backtrace) to a persistent file.
- Capture raw stderr from C-level components (GTK, GLib, libghostty) to a
  persistent file, so GLib critical warnings that typically precede these
  crashes survive.
- Emit structured `tracing` events at UI-level operations most likely to be
  implicated (paste, split, tab reorder, workspace switch, browser navigate,
  terminal spawn/exit) so the log contains breadcrumbs immediately before a
  crash line.
- Rotate daily, keep 7 days, so the file never grows unboundedly and
  "yesterday at 3pm" is trivial to locate.
- No privacy regressions: never log keystrokes, clipboard contents, terminal
  I/O, browser page contents, query strings, or auth material.

## Non-goals

- Replacing `coredumpctl` as the source of post-crash stack traces for C-level
  segfaults. The stderr log is a breadcrumb trail; `coredumpctl` remains the
  authoritative backtrace tool.
- Extending logging to `chostty-cli`, `chostty-control`, `chostty-core`, or
  `chostty-protocol`. Those can adopt `tracing` later if a need appears;
  the host is the only crate that crashes daily.
- A remote log sink, log shipping, or any network egress.
- A SIGSEGV signal handler. Core dumps already provide the real stack trace
  with symbols; a hand-rolled handler adds complexity for little gain.

## Architecture

### Location

- Log directory: `$XDG_STATE_HOME/chostty/` with `~/.local/state/chostty/`
  fallback when `XDG_STATE_HOME` is unset. XDG specifies `STATE` as the
  correct category for persistent log files.
- `chostty.log` — structured `tracing` events. Daily rotation via
  `tracing-appender::rolling::daily`, 7 files retained.
- `chostty.stderr.log` — raw bytes written to fd 2 after `dup2`. Truncated
  on each launch, so each file corresponds to a single process lifetime.

### Module

A new `rust/chostty-host-linux/src/logging.rs` module called once from the
top of `main()` in `rust/chostty-host-linux/src/main.rs`, before any GTK or
Ghostty initialization, so warnings emitted during those subsystems'
startup are captured.

### Dependencies

Added to `rust/chostty-host-linux/Cargo.toml` only:

- `tracing = "0.1"`
- `tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }`
- `tracing-appender = "0.2"`
- `libc = "0.2"` (direct dep for `dup2`)

No workspace-level dependency changes.

## Components

Five functions in `logging.rs`:

1. **`pub fn init()`** — the public entry point. Orchestrates the four steps
   below. Returns `()`; the blocking writer needs no flush guard (see
   rationale below).
2. **`fn log_dir() -> PathBuf`** — resolves `$XDG_STATE_HOME/chostty` with
   `~/.local/state/chostty/` fallback. Creates the directory with
   `fs::create_dir_all`.
3. **`fn redirect_stderr(dir: &Path) -> io::Result<()>`** — opens
   `chostty.stderr.log` with `O_TRUNC | O_CREAT | O_WRONLY`, writes a
   session banner (`=== chostty v<version> pid=<pid> started <UTC iso-8601> ===`),
   calls `libc::dup2(file_fd, 2)`, then deliberately leaks the `File` so the
   fd stays valid for the process lifetime. Without the leak, Rust would
   close the backing fd at end of scope even though fd 2 still refers to it
   via the kernel fd table.
4. **`fn init_tracing(dir: &Path)`** — builds
   `tracing_appender::rolling::daily(dir, "chostty.log")` and uses it
   directly as the `fmt` layer's writer (blocking path — no `non_blocking`
   wrapper, no background worker). Sets an `EnvFilter` defaulting to `info`
   and respecting `RUST_LOG`, and installs as the global subscriber. No
   guard returned; with the blocking writer there is nothing to flush on
   drop.
5. **`fn install_panic_hook()`** — chains a hook in front of Rust's default.
   The new hook emits a `tracing::error!` with the panic payload, location,
   and `std::backtrace::Backtrace::force_capture()`, then calls the
   previously installed hook so the default stderr-printing behavior still
   fires (which now lands in `chostty.stderr.log` via the redirect).

### Why blocking writer, not `non_blocking`

`tracing-appender`'s `non_blocking` uses a background worker thread. When a
segfault in C code aborts the process, `Drop` for the `WorkerGuard` does not
run, so buffered events are lost. For crash diagnosis the most valuable line
is the *last* one before the fault. The blocking writer writes synchronously
on each event, so anything logged before the fault is on disk. The
performance cost is negligible for a GUI app that logs user-triggered
events, not high-frequency telemetry.

## Instrumentation sites

Exact file:line locations pinned during writing-plans. Target sites:

### INFO-level (always on)

- `main.rs::main` — startup (version, `XDG_STATE_HOME` resolution, log file
  paths); shutdown.
- `window.rs` — workspace created / closed / switched / renamed / reordered.
- `pane.rs` or `split_tree.rs` — pane split (direction), pane closed.
- `terminal.rs` — tab new / closed / reordered / switched; terminal spawned
  (shell path, cwd); terminal exited (exit code, duration).
- Paste sites — source (clipboard / primary / bracketed), byte count only
  (never contents).
- `open_path_dialog` — path chosen (not keystrokes).
- Browser navigate — URL host only (never query string, never full URL).

### DEBUG-level (`RUST_LOG=chostty=debug`)

- Focus changes, keybind fires, drag-and-drop steps, settings open/close,
  control socket messages.

### Not logged (privacy)

Keystrokes, clipboard contents, terminal I/O, browser page contents, query
strings, auth headers, passwords.

### Style

Prefer `#[tracing::instrument(skip(...))]` on methods where it fits. Use
field-style events (`info!(event = "paste", bytes = n)`) rather than
interpolated strings, so the log is grep-friendly.

## Data flow

1. `main()` first line: `logging::init();`.
2. `init()` resolves and creates the log dir → truncates and opens
   `chostty.stderr.log` → writes session banner → `dup2(fd, 2)` → builds
   rolling daily appender for `chostty.log` → installs `tracing_subscriber`
   with `EnvFilter` → installs panic hook → returns `_log_guard`.
3. At runtime:
   - `tracing::info!(...)` → subscriber → appender → `chostty.log`.
   - `fprintf(stderr, ...)` from GTK/GLib/libghostty → fd 2 →
     `chostty.stderr.log`.
   - Rust panic → our hook logs via `tracing` → default hook prints to
     (redirected) stderr → both files record it.
4. Segfault in C: process dies. Because tracing is blocking, all events
   emitted before the fault are already on disk. glibc line-buffers stderr
   for fd 2, so GLib criticals are also on disk. `coredumpctl` retains the
   real stack trace.
5. Clean exit: nothing to flush. The blocking writer has already written
   each event synchronously.

### Daily rollover edge case

At midnight, `tracing-appender` opens a new `chostty.log` for today while
the raw stderr fd still points at the previous day's `chostty.stderr.log`.
This is intentional — `chostty.stderr.log` is session-scoped by design, so
no mid-session rotation is wanted. The structured `chostty.log` rotates
correctly via the appender's internal fd management.

## Error handling

Fail-soft at every boundary. Logging failure must not prevent the app from
running.

- `log_dir` creation fails → emit one line to the real (pre-redirect) stderr,
  skip the redirect, install a stderr-only `tracing_subscriber::fmt`. App
  runs; user sees no file logs but the app is usable.
- `dup2` fails → emit a warning to the real stderr, keep the original
  stderr, continue with tracing-to-file only.
- `tracing-appender` initialization fails → fall back to
  `tracing_subscriber::fmt` pointed at stderr. App runs.
- No `unwrap` or `expect` on any of these paths.

## Testing

### Unit tests (`#[cfg(test)]` inside `logging.rs`)

- `log_dir()` returns `$XDG_STATE_HOME/chostty` when `XDG_STATE_HOME` is set.
- `log_dir()` returns `~/.local/state/chostty` when `XDG_STATE_HOME` is
  unset (use a `tempfile::tempdir` + `HOME` override pattern).
- `EnvFilter` construction: defaults to `info`, respects
  `RUST_LOG=chostty=debug`.

### Manual verification (documented in the spec)

- Add a debug-only keybind or menu entry that calls `panic!("test panic")`;
  trigger it; confirm panic message, location, and backtrace appear in
  `chostty.log`, and that the process still writes to
  `chostty.stderr.log` (default panic hook's stderr output).
- Run with `G_DEBUG=fatal-warnings` against a known GLib warning; confirm
  the warning is in `chostty.stderr.log`.
- Launch twice in succession; confirm `chostty.stderr.log` is truncated on
  the second launch (only second session's banner + content present).
- Either leave a build running across midnight or temporarily set the
  system clock forward; confirm daily rollover produces
  `chostty.log.YYYY-MM-DD` files and that files older than 7 days are
  pruned.
- Set `RUST_LOG=chostty=debug` and confirm DEBUG-level events from the
  instrumentation sites appear.

No automated test for `dup2` side effects — that requires a subprocess
harness. Manual verification above is sufficient.

## Open questions

None at the design stage. Specific file:line instrumentation sites will be
enumerated in the implementation plan.
