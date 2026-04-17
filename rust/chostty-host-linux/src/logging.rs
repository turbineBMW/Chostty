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
