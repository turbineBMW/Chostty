# Ghostty-style Overlay Scrollbars for Chostty

**Date:** 2026-04-17
**Branch:** `ghostty-scroll-bars`
**Status:** Design approved — ready for implementation planning

## Problem

Chostty currently has no scrollbar on its terminal panes. Upstream Ghostty has a pleasant scrollbar experience: a thin, semi-transparent indicator fades in when the user scrolls or moves the pointer, the track appears and the handle widens when the pointer hovers the handle, and the indicator fades back out on idle.

The goal is to bring that same experience to Chostty.

## Key insight

Upstream Ghostty's scrollbar look-and-feel is **not a custom widget** — it is GTK 4's built-in overlay-scrollbar behavior on `Gtk.ScrolledWindow`. Ghostty's Surface widget implements the `Gtk.Scrollable` interface and is wrapped in a `Gtk.ScrolledWindow`; GTK handles the fade-in, thin/widen-on-hover, and auto-hide animations.

The libghostty core emits a `GHOSTTY_ACTION_SCROLLBAR` event on scroll-state changes, and a `scroll_to_row:<n>` binding action lets a host move the scroll position. Chostty already uses `ghostty_surface_binding_action` for other commands (`window.rs:4317–4333`), so the plumbing is cheap.

## Architecture

### Widget tree (per terminal)

Current:
```
Overlay > GLArea
        > SearchBar (overlay child)
        > Toast (overlay child)
```

New:
```
Overlay > ScrolledWindow > ChosttyTerminalArea (GLArea + Scrollable)
        > SearchBar  (overlay child)
        > Toast      (overlay child)
```

### New widget: `ChosttyTerminalArea`

A gtk-rs subclass of `gtk::GLArea` that implements the `gtk::Scrollable` interface. The implementation is pure property plumbing:

- `hadjustment`, `vadjustment`: `Option<gtk::Adjustment>` stored in a `RefCell`; getter/setter just read/write the field and notify.
- `hscroll_policy`, `vscroll_policy`: `gtk::ScrollablePolicy` stored similarly.

No custom layout logic. The `Scrollable` interface exists purely so `GtkScrolledWindow` can install its adjustments on the widget.

### `ScrolledWindow` configuration

- `hscrollbar_policy = Never` (terminals don't horizontally scroll)
- `vscrollbar_policy = <value from config>` — see *Config* section
- `overlay_scrolling = true` (default) — drives the fade / thin / widen behavior
- `kinetic_scrolling = false` — terminal scrollback is discrete-row; kinetic scrolling feels wrong and matches upstream's workaround

### FFI additions — `chostty-ghostty-sys/src/lib.rs`

```rust
pub const GHOSTTY_ACTION_SCROLLBAR: c_int = 26;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ghostty_action_scrollbar_s {
    pub total: u64,
    pub offset: u64,
    pub len: u64,
}
```

Extend `ghostty_action_u` with a `scrollbar: ghostty_action_scrollbar_s` field. Three `u64`s = 24 bytes, fitting the existing 24-byte padding exactly.

Add a compile-time assertion (`#[test]` with `size_of`) that `ghostty_action_scrollbar_s` is 24 bytes, mirroring the pattern used for other FFI structs in the crate.

## Data flow

### Core → UI (pty output updates the scrollbar)

1. libghostty emits `GHOSTTY_ACTION_SCROLLBAR` with `{total, offset, len}`.
2. The action callback (`terminal.rs:457`) gets a new arm:
   ```rust
   GHOSTTY_ACTION_SCROLLBAR => {
       if target.tag == GHOSTTY_TARGET_SURFACE {
           let surface_key = unsafe { target.target.surface } as usize;
           let s = unsafe { action.action.scrollbar };
           SURFACE_MAP.with(|map| {
               if let Some(entry) = map.borrow().get(&surface_key) {
                   apply_scrollbar(entry, s);
               }
           });
       }
       true
   }
   ```
3. `apply_scrollbar` fetches the `vadjustment` from the `ChosttyTerminalArea`, sets a `suppress_value_changed` flag on the entry to `true`, calls `adj.configure(offset as f64, 0.0, total as f64, 1.0, len as f64, len as f64)`, then clears the flag. This mirrors upstream `surface.zig:960–999`.

### UI → core (user drags the scrollbar)

1. On `ChosttyTerminalArea` construction, connect a `value-changed` handler to its `vadjustment`:
   ```rust
   vadj.connect_value_changed(clone!(@weak surface_cell, @weak suppress => move |adj| {
       if suppress.get() { return; }
       let row = adj.value().round() as usize;
       let action = format!("scroll_to_row:{row}");
       if let Some(surface) = *surface_cell.borrow() {
           unsafe { ghostty_surface_binding_action(surface, action.as_ptr() as _, action.len()) };
       }
   }));
   ```
2. The `suppress` flag is the same one set during programmatic `configure` — prevents feedback loops.

### Mouse wheel (unchanged)

The existing `EventControllerScroll` at `terminal.rs:1449` stays on the inner `ChosttyTerminalArea` and returns `glib::Propagation::Stop`. libghostty handles wheel events internally — including modifier-aware variants like `Ctrl+scroll` for font size — then emits `ACTION_SCROLLBAR`, which updates the adjustment, which causes GTK to fade the overlay scrollbar in. No double-scrolling; modifier+wheel behavior preserved.

## Config & lifecycle

### Reading `scrollbar` from Ghostty config

libghostty's `Config.zig:1403` defines `scrollbar: Scrollbar = .system`, with values `.system` and `.never`. `ghostty_config_get` returns enum values as **null-terminated strings** (per `c_get.zig:62–65` — `ptr.* = @tagName(value)`), not as discriminants.

```rust
fn scrollbar_policy_from_config(config: ghostty_config_t) -> gtk::PolicyType {
    let mut out: *const c_char = std::ptr::null();
    let ok = unsafe {
        ghostty_config_get(
            config,
            &mut out as *mut _ as *mut c_void,
            b"scrollbar".as_ptr() as *const c_char,
            "scrollbar".len(),
        )
    };
    if !ok || out.is_null() {
        return gtk::PolicyType::Automatic;
    }
    let s = unsafe { std::ffi::CStr::from_ptr(out) }.to_bytes();
    match s {
        b"never" => gtk::PolicyType::Never,
        _ => gtk::PolicyType::Automatic, // "system" and any unknown value
    }
}
```

The returned string is owned by the config — do not free it. String comparison is also forward-compatible if Ghostty adds new enum variants.

### Initial apply

At terminal construction, read the policy once and call `scrolled_window.set_vscrollbar_policy(policy)`.

### Reload on config change

The existing `GHOSTTY_ACTION_RELOAD_CONFIG` handler at `terminal.rs:575` already reloads config for each surface. Extend it: after `ghostty_surface_update_config`, re-read the scrollbar policy and apply it to the surface's `ScrolledWindow`. This requires storing a ref to the `ScrolledWindow` in each `SURFACE_MAP` entry alongside `gl_area`.

### Lifetime

- `ScrolledWindow` is a child of the existing `TerminalWidget.overlay` — no new lifecycle.
- `ChosttyTerminalArea` drops its adjustment refs automatically via gtk-rs property cleanup.
- `SURFACE_MAP` gets one new field (`scrolled_window`) and one new flag (`suppress_value_changed`). Entry removal on pane close already covers them.

## Error handling & edge cases

- **Invalid/unknown surface in action callback** → silently dropped by the existing `SURFACE_MAP.get()` guard.
- **`vadjustment` is `None` during teardown** → the programmatic-update path checks `Option` and returns early (matches upstream `setScrollbar` at `surface.zig:966`).
- **Config read returns an unexpected enum discriminant** → default to `PolicyType::Automatic`.
- **Zero-sized adjustment (`total == 0`, `len == 0`)** → GTK auto-hides the overlay when `page_size >= upper`; no custom logic needed.
- **Reentrancy between our `configure` call and `value-changed`** → guarded by the `suppress_value_changed: Cell<bool>` on each surface map entry. Upstream uses a `SignalGroup.block()`; `Cell<bool>` is lighter and fine in Rust since all access is main-thread.
- **No new thread-safety surface** — Ghostty's runtime contract puts action callbacks on the GTK main thread.

## Out of scope (YAGNI)

- No horizontal scrollbar support; `hadjustment` is plumbed for the interface only.
- No Chostty-specific scrollbar settings in the Settings UI — users configure via `~/.config/ghostty/config`.
- No kinetic-scroll workaround for GTK < 4.20.1. Chostty requires libadwaita ≥ 1.5 (GTK ≥ 4.14); we unconditionally set `kinetic_scrolling = false` on the ScrolledWindow, which is the correct behavior for a terminal regardless.
- No custom CSS theming. GTK's overlay scrollbar appearance under the active libadwaita theme is what the user explicitly asked for.

## Testing

### Automated

- Unit test the `scrollbar_policy_from_config` mapping (pure function over enum discriminant → `PolicyType`).
- FFI struct-size assertion: `assert_eq!(size_of::<ghostty_action_scrollbar_s>(), 24);`.

### Manual verification checklist

1. Build (`cargo build --release` with the usual `LD_LIBRARY_PATH`) and launch Chostty.
2. Open a terminal pane; run `seq 10000` to fill scrollback.
3. Wheel-scroll up: thin, semi-transparent indicator fades in on the right edge.
4. Hover the handle: track appears, handle widens.
5. Drag the handle: terminal viewport moves; releasing holds the position.
6. Scroll to the bottom: indicator fades out after a moment.
7. Split the pane (`Ctrl+D` / `Ctrl+Shift+D`): each pane gets its own independent scrollbar.
8. `Ctrl+scroll`: font size still changes (wheel not eaten by ScrolledWindow).
9. Set `scrollbar = never` in `~/.config/ghostty/config`, relaunch: no scrollbar appears at all.
10. Close a scrolled-back pane: no crash, no orphaned widgets.

## Files touched (expected)

- `rust/chostty-ghostty-sys/src/lib.rs` — add `GHOSTTY_ACTION_SCROLLBAR` constant, `ghostty_action_scrollbar_s` struct, union field, size assertion test.
- `rust/chostty-host-linux/src/terminal.rs`:
  - Define `ChosttyTerminalArea` (GLArea subclass + Scrollable impl).
  - Swap `GLArea::new()` for `ChosttyTerminalArea::new()`.
  - Wrap in `ScrolledWindow`; make it the `Overlay` child.
  - Add `SCROLLBAR` arm to action callback; `apply_scrollbar` helper.
  - Connect `vadjustment.value-changed` → `scroll_to_row:<n>`.
  - `scrollbar_policy_from_config` helper; apply at construction and on `RELOAD_CONFIG`.
  - Extend `SURFACE_MAP` entry with `scrolled_window` ref and `suppress_value_changed: Cell<bool>`.

No changes expected in: `pane.rs`, `window.rs`, `split_tree.rs`, other crates.
