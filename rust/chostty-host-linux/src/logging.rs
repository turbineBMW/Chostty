//! Persistent file logging for chostty-host-linux.
//!
//! See docs/superpowers/specs/2026-04-17-persistent-logging-design.md.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing_appender::rolling::{Builder, Rotation};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

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
