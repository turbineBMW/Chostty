# Ghostty-Style Overlay Scrollbars — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add GTK 4 overlay scrollbars (fade-in, thin, widen-on-hover) to every Chostty terminal pane, matching upstream Ghostty's behavior.

**Architecture:** Subclass `gtk::GLArea` with a custom widget (`ChosttyTerminalArea`) that implements `gtk::Scrollable`. Wrap it in a `gtk::ScrolledWindow` inside the existing per-terminal `gtk::Overlay`. Drive the ScrolledWindow's vertical adjustment from libghostty's `GHOSTTY_ACTION_SCROLLBAR` action and drive user drags back into libghostty via `ghostty_surface_binding_action(s, "scroll_to_row:<n>", ...)`.

**Tech Stack:** Rust 1.x, gtk-rs 0.11 (`gtk4` crate, feature `v4_10`), libadwaita 0.9, Chostty's existing FFI crate (`chostty-ghostty-sys`), libghostty C API.

**Spec:** `docs/superpowers/specs/2026-04-17-ghostty-scrollbars-design.md`

---

## File structure

- `rust/chostty-ghostty-sys/src/lib.rs` — add `GHOSTTY_ACTION_SCROLLBAR`, `ghostty_action_scrollbar_s`, union field, size assertion.
- `rust/chostty-host-linux/src/terminal.rs` — add `ChosttyTerminalArea` subclass module, helper, widget swap, action arm, value-changed handler, reload-config integration, map-entry changes.

No new files. All changes localized.

---

## Task 1: FFI — add scrollbar action constant, struct, and union field

**Files:**
- Modify: `rust/chostty-ghostty-sys/src/lib.rs`

- [ ] **Step 1.1: Add a size-assertion test for the new struct (failing — struct doesn't exist yet)**

Append to `rust/chostty-ghostty-sys/src/lib.rs` (at end of file):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::size_of;

    #[test]
    fn scrollbar_struct_is_24_bytes() {
        // Must match C: struct { uint64_t total; uint64_t offset; uint64_t len; }
        assert_eq!(size_of::<ghostty_action_scrollbar_s>(), 24);
    }

    #[test]
    fn action_union_is_24_bytes() {
        // The scrollbar struct must not grow the union.
        assert_eq!(size_of::<ghostty_action_u>(), 24);
    }
}
```

- [ ] **Step 1.2: Run the test to verify it fails**

Run:
```bash
cd rust/chostty-ghostty-sys && cargo test
```
Expected: **FAIL** with `error[E0412]: cannot find type 'ghostty_action_scrollbar_s'`.

- [ ] **Step 1.3: Add the action tag constant**

In `rust/chostty-ghostty-sys/src/lib.rs`, find the line:

```rust
pub const GHOSTTY_ACTION_RENDER: c_int = 27;
```

Immediately **before** it, add:

```rust
pub const GHOSTTY_ACTION_SCROLLBAR: c_int = 26;
```

- [ ] **Step 1.4: Add the scrollbar struct**

In the same file, find the block of existing action structs (around `ghostty_action_pwd_s`, `ghostty_surface_message_childexited_s`). After the last `#[repr(C)]` action struct in that block and before the `// Runtime config (callbacks)` comment, add:

```rust
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ghostty_action_scrollbar_s {
    pub total: u64,
    pub offset: u64,
    pub len: u64,
}
```

- [ ] **Step 1.5: Add the scrollbar field to the action union**

Find the existing `ghostty_action_u` definition (around line 295):

```rust
#[repr(C)]
pub union ghostty_action_u {
    pub desktop_notification: ghostty_action_desktop_notification_s,
    pub set_title: ghostty_action_set_title_s,
    pub pwd: ghostty_action_pwd_s,
    pub child_exited: ghostty_surface_message_childexited_s,
    _padding: [u8; 24],
}
```

Add `pub scrollbar: ghostty_action_scrollbar_s,` as a new field, immediately after `pub child_exited: ghostty_surface_message_childexited_s,`:

```rust
#[repr(C)]
pub union ghostty_action_u {
    pub desktop_notification: ghostty_action_desktop_notification_s,
    pub set_title: ghostty_action_set_title_s,
    pub pwd: ghostty_action_pwd_s,
    pub child_exited: ghostty_surface_message_childexited_s,
    pub scrollbar: ghostty_action_scrollbar_s,
    _padding: [u8; 24],
}
```

(Three `u64`s = 24 bytes, so the `_padding` still caps union size at 24.)

- [ ] **Step 1.6: Run the tests to verify they pass**

Run:
```bash
cd rust/chostty-ghostty-sys && cargo test
```
Expected: **PASS** — both `scrollbar_struct_is_24_bytes` and `action_union_is_24_bytes`.

- [ ] **Step 1.7: Commit**

```bash
git add rust/chostty-ghostty-sys/src/lib.rs
git commit -m "$(cat <<'EOF'
Add GHOSTTY_ACTION_SCROLLBAR FFI binding

Expose the scrollbar action (tag 26) and its payload struct
{total, offset, len} so the host can receive scrollback state
updates from libghostty.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: Add the config → `PolicyType` helper

**Files:**
- Modify: `rust/chostty-ghostty-sys/src/lib.rs` — add `ghostty_config_get` extern.
- Modify: `rust/chostty-host-linux/src/terminal.rs`

- [ ] **Step 2.1: Confirm `ghostty_config_get` extern exists in the sys crate**

Run:
```bash
grep -n "ghostty_config_get" rust/chostty-ghostty-sys/src/lib.rs
```

Expected: one or more hits. If there's already a binding for `ghostty_config_get`, skip to step 2.3. If not, continue.

- [ ] **Step 2.2 (only if the extern is missing): Add the `ghostty_config_get` FFI binding**

In `rust/chostty-ghostty-sys/src/lib.rs`, find the `extern "C" {` block that declares other `ghostty_config_*` functions (near line 370) and add inside it:

```rust
    pub fn ghostty_config_get(
        config: ghostty_config_t,
        out: *mut c_void,
        key: *const c_char,
        key_len: usize,
    ) -> bool;
```

Build to confirm:
```bash
cd rust/chostty-ghostty-sys && cargo build
```
Expected: clean build.

- [ ] **Step 2.3: Add the pure tag-name → PolicyType helper + failing test**

In `rust/chostty-host-linux/src/terminal.rs`, at the **end** of the file (after the last item), add:

```rust
/// Map a `scrollbar` config enum tag name (as returned by `ghostty_config_get`)
/// to a GTK scrollbar visibility policy. Unknown/invalid values default to
/// `Automatic`, matching upstream Ghostty's `closureScrollbarPolicy` behavior.
fn scrollbar_policy_from_tag(tag: &[u8]) -> gtk::PolicyType {
    match tag {
        b"never" => gtk::PolicyType::Never,
        _ => gtk::PolicyType::Automatic,
    }
}

#[cfg(test)]
mod scrollbar_policy_tests {
    use super::scrollbar_policy_from_tag;
    use gtk::PolicyType;

    #[test]
    fn system_maps_to_automatic() {
        assert_eq!(scrollbar_policy_from_tag(b"system"), PolicyType::Automatic);
    }

    #[test]
    fn never_maps_to_never() {
        assert_eq!(scrollbar_policy_from_tag(b"never"), PolicyType::Never);
    }

    #[test]
    fn unknown_defaults_to_automatic() {
        assert_eq!(scrollbar_policy_from_tag(b"garbage"), PolicyType::Automatic);
        assert_eq!(scrollbar_policy_from_tag(b""), PolicyType::Automatic);
    }
}
```

- [ ] **Step 2.4: Run the unit tests to verify they pass**

Run:
```bash
cd rust/chostty-host-linux && cargo test scrollbar_policy
```
Expected: three PASS — `system_maps_to_automatic`, `never_maps_to_never`, `unknown_defaults_to_automatic`.

If `cargo test` fails to link at this point due to libghostty symbols, try:
```bash
LD_LIBRARY_PATH=../../ghostty/zig-out/lib:$LD_LIBRARY_PATH cargo test scrollbar_policy
```
(Chostty tests depend on `libghostty.so`; the env var may already be set via `.cargo/config.toml` — check via `cargo test scrollbar_policy` first.)

- [ ] **Step 2.5: Add the FFI wrapper helper**

Directly **after** `scrollbar_policy_from_tag` (before the `#[cfg(test)]` block), add:

```rust
/// Read the `scrollbar` key from a libghostty `ghostty_config_t` and return
/// the corresponding GTK scrollbar policy. On any read failure we default to
/// `Automatic` (system default).
///
/// The returned string is owned by the config — do not free it.
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
    let tag = unsafe { std::ffi::CStr::from_ptr(out) }.to_bytes();
    scrollbar_policy_from_tag(tag)
}
```

If `ghostty_config_t`, `ghostty_config_get`, `c_char`, or `c_void` are not in scope at the top of `terminal.rs`, add them to the existing `use chostty_ghostty_sys::{...};` or `use std::...;` imports.

- [ ] **Step 2.6: Verify it compiles**

Run:
```bash
cd rust/chostty-host-linux && cargo build
```
Expected: clean build (the helper is added but not yet called, so it may warn "function is never used" — that's fine until Task 5/8).

To silence the unused-function warning for now, temporarily annotate it:

```rust
#[allow(dead_code)]
fn scrollbar_policy_from_config(config: ghostty_config_t) -> gtk::PolicyType {
```

We'll remove `#[allow(dead_code)]` in Task 5 when it's first called. Same for `scrollbar_policy_from_tag`:

```rust
#[allow(dead_code)]
fn scrollbar_policy_from_tag(tag: &[u8]) -> gtk::PolicyType {
```

- [ ] **Step 2.7: Run fmt + clippy + tests**

```bash
./scripts/check.sh
```

Expected: clean pass.

- [ ] **Step 2.8: Commit**

```bash
git add rust/chostty-host-linux/src/terminal.rs rust/chostty-ghostty-sys/src/lib.rs
git commit -m "$(cat <<'EOF'
Add scrollbar_policy_from_config helper

Reads the `scrollbar` key from libghostty config (returned as a
C string per c_get.zig) and maps to gtk::PolicyType. Separate the
pure string→policy mapping from the FFI wrapper so the mapping is
unit-testable without a live libghostty config.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Create the `ChosttyTerminalArea` custom widget

This task creates a `gtk::GLArea` subclass that implements `gtk::Scrollable`. No functional change yet — we introduce the type and swap it in Task 5.

**Files:**
- Modify: `rust/chostty-host-linux/src/terminal.rs`

- [ ] **Step 3.1: Add the widget module at the top of `terminal.rs` (after the existing `use` block)**

Find the block of top-level `use` statements. After the last `use ...;` and before the next non-`use` item, add:

```rust
// ---------------------------------------------------------------------------
// ChosttyTerminalArea — a GLArea subclass that implements Gtk.Scrollable.
//
// The Scrollable interface is pure property plumbing: we just store the
// four properties (hadjustment, vadjustment, hscroll-policy, vscroll-policy)
// so that a surrounding GtkScrolledWindow can install its adjustments on
// us. GTK handles the rest — including the overlay-scrollbar fade-in /
// widen-on-hover behavior we want.
// ---------------------------------------------------------------------------

mod terminal_area {
    use std::cell::{Cell, RefCell};

    use glib::subclass::prelude::*;
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;
    use gtk::{gio, glib};

    #[derive(Default)]
    pub struct ChosttyTerminalAreaPriv {
        pub hadjustment: RefCell<Option<gtk::Adjustment>>,
        pub vadjustment: RefCell<Option<gtk::Adjustment>>,
        pub hscroll_policy: Cell<gtk::ScrollablePolicy>,
        pub vscroll_policy: Cell<gtk::ScrollablePolicy>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ChosttyTerminalAreaPriv {
        const NAME: &'static str = "ChosttyTerminalArea";
        type Type = super::ChosttyTerminalArea;
        type ParentType = gtk::GLArea;
        type Interfaces = (gtk::Scrollable,);
    }

    impl ObjectImpl for ChosttyTerminalAreaPriv {
        fn properties() -> &'static [glib::ParamSpec] {
            use std::sync::OnceLock;
            static PROPERTIES: OnceLock<Vec<glib::ParamSpec>> = OnceLock::new();
            PROPERTIES.get_or_init(|| {
                vec![
                    glib::ParamSpecOverride::for_interface::<gtk::Scrollable>("hadjustment"),
                    glib::ParamSpecOverride::for_interface::<gtk::Scrollable>("vadjustment"),
                    glib::ParamSpecOverride::for_interface::<gtk::Scrollable>("hscroll-policy"),
                    glib::ParamSpecOverride::for_interface::<gtk::Scrollable>("vscroll-policy"),
                ]
            })
        }

        fn set_property(&self, _id: usize, value: &glib::Value, pspec: &glib::ParamSpec) {
            match pspec.name() {
                "hadjustment" => {
                    let adj: Option<gtk::Adjustment> = value.get().unwrap();
                    *self.hadjustment.borrow_mut() = adj;
                }
                "vadjustment" => {
                    let adj: Option<gtk::Adjustment> = value.get().unwrap();
                    *self.vadjustment.borrow_mut() = adj;
                }
                "hscroll-policy" => {
                    let p: gtk::ScrollablePolicy = value.get().unwrap();
                    self.hscroll_policy.set(p);
                }
                "vscroll-policy" => {
                    let p: gtk::ScrollablePolicy = value.get().unwrap();
                    self.vscroll_policy.set(p);
                }
                _ => unreachable!("unknown Scrollable property: {}", pspec.name()),
            }
        }

        fn property(&self, _id: usize, pspec: &glib::ParamSpec) -> glib::Value {
            match pspec.name() {
                "hadjustment" => self.hadjustment.borrow().to_value(),
                "vadjustment" => self.vadjustment.borrow().to_value(),
                "hscroll-policy" => self.hscroll_policy.get().to_value(),
                "vscroll-policy" => self.vscroll_policy.get().to_value(),
                _ => unreachable!("unknown Scrollable property: {}", pspec.name()),
            }
        }
    }

    impl WidgetImpl for ChosttyTerminalAreaPriv {}
    impl GLAreaImpl for ChosttyTerminalAreaPriv {}
    impl ScrollableImpl for ChosttyTerminalAreaPriv {}

    // Silence an unused-import warning if gio ends up not being referenced.
    #[allow(dead_code)]
    fn _keep_imports_alive(_: gio::Cancellable) {}
}

glib::wrapper! {
    /// A GLArea subclass that implements `Gtk.Scrollable`. Otherwise behaves
    /// exactly like `gtk::GLArea` for our host-side usage.
    pub struct ChosttyTerminalArea(ObjectSubclass<terminal_area::ChosttyTerminalAreaPriv>)
        @extends gtk::GLArea, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Scrollable;
}

impl Default for ChosttyTerminalArea {
    fn default() -> Self {
        glib::Object::new()
    }
}

impl ChosttyTerminalArea {
    pub fn new() -> Self {
        Self::default()
    }
}
```

**Notes for the implementer:**
- `ParamSpecOverride::for_interface` is the standard gtk-rs 0.11 way to declare "this object implements interface property X". See `gtk-rs` examples for `Scrollable`.
- If `glib::ParamSpecOverride::for_interface` is not available under that exact path in the installed gtk-rs 0.11, use `glib::ParamSpec::builder_with_override("hadjustment", gtk::Scrollable::static_type())` pattern. Check `cargo doc --package gtk4 --open` → `gtk::Scrollable`.
- `Interfaces = (gtk::Scrollable,)` tells gobject to register us as implementing the interface; without this the property overrides fail.

- [ ] **Step 3.2: Verify the module compiles**

Run:
```bash
cd rust/chostty-host-linux && cargo build
```
Expected: clean compile (the new type is unused but declared; expect one "unused" warning if `#[allow(dead_code)]` is not on it — add `#[allow(dead_code)]` on `impl ChosttyTerminalArea` temporarily if needed).

If it **fails to compile**, most likely causes:
- `ParamSpecOverride::for_interface` path is different in your gtk-rs version. Try the builder form:
  ```rust
  glib::ParamSpec::builder("hadjustment")
      .override_interface::<gtk::Scrollable>()
      .build()
  ```
  Or look at gtk-rs's own `examples/` directory for a Scrollable-implementing widget.
- `ScrollablePolicy::default()` not impl'd — wrap `Cell<gtk::ScrollablePolicy>` as `Cell<gtk::ScrollablePolicy>` with an explicit Default: add a `Default` manual impl on `ChosttyTerminalAreaPriv` that initializes `hscroll_policy` / `vscroll_policy` to `gtk::ScrollablePolicy::Minimum`:
  ```rust
  impl Default for ChosttyTerminalAreaPriv {
      fn default() -> Self {
          Self {
              hadjustment: RefCell::new(None),
              vadjustment: RefCell::new(None),
              hscroll_policy: Cell::new(gtk::ScrollablePolicy::Minimum),
              vscroll_policy: Cell::new(gtk::ScrollablePolicy::Minimum),
          }
      }
  }
  ```
  Remove `#[derive(Default)]` from the struct if you do this.

- [ ] **Step 3.3: Smoke-test property plumbing with a unit test**

Append to the existing `#[cfg(test)] mod scrollbar_policy_tests` block (or create a new test mod at end of file):

```rust
#[cfg(test)]
mod terminal_area_tests {
    use super::ChosttyTerminalArea;
    use gtk::prelude::*;

    #[test]
    fn vadjustment_round_trips() {
        gtk::init().expect("gtk init");
        let area = ChosttyTerminalArea::new();
        assert!(ScrollableExt::vadjustment(&area).is_none());

        let adj = gtk::Adjustment::new(0.0, 0.0, 100.0, 1.0, 10.0, 10.0);
        ScrollableExt::set_vadjustment(&area, Some(&adj));
        let got = ScrollableExt::vadjustment(&area).expect("vadj set");
        assert_eq!(got.upper(), 100.0);
        assert_eq!(got.page_size(), 10.0);
    }
}
```

- [ ] **Step 3.4: Run the smoke test**

```bash
cd rust/chostty-host-linux && cargo test vadjustment_round_trips
```
Expected: PASS. (If it fails with `gtk::init()` errors because no display is available, skip the test for now by adding `#[ignore]` — the visual verification in Task 8 will cover it. Comment near the `#[test]` explaining why.)

If gtk::init fails in CI or headless, guard:
```rust
if gtk::init().is_err() { return; }
```

- [ ] **Step 3.5: Commit**

```bash
git add rust/chostty-host-linux/src/terminal.rs
git commit -m "$(cat <<'EOF'
Add ChosttyTerminalArea: GLArea subclass implementing Gtk.Scrollable

Pure property plumbing — stores hadjustment/vadjustment and the two
scroll-policy enums so a surrounding GtkScrolledWindow can install
adjustments on us. No functional change yet; Task 5 swaps the raw
GLArea construction for this type.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Extend `SurfaceEntry` with scrolled-window ref and reentrancy flag

**Files:**
- Modify: `rust/chostty-host-linux/src/terminal.rs`

- [ ] **Step 4.1: Update the `SurfaceEntry` struct (line ~43)**

Find:

```rust
struct SurfaceEntry {
    gl_area: gtk::GLArea,
    toast_overlay: gtk::Overlay,
    on_title_changed: Option<Box<TitleChangedCallback>>,
    on_pwd_changed: Option<Box<PwdChangedCallback>>,
    on_desktop_notification: Option<Box<DesktopNotificationCallback>>,
    on_bell: Option<Box<VoidCallback>>,
    on_close: Option<Box<VoidCallback>>,
    clipboard_context: *mut ClipboardContext,
}
```

Replace with:

```rust
struct SurfaceEntry {
    gl_area: ChosttyTerminalArea,
    scrolled_window: gtk::ScrolledWindow,
    suppress_vadj_signal: Rc<Cell<bool>>,
    toast_overlay: gtk::Overlay,
    on_title_changed: Option<Box<TitleChangedCallback>>,
    on_pwd_changed: Option<Box<PwdChangedCallback>>,
    on_desktop_notification: Option<Box<DesktopNotificationCallback>>,
    on_bell: Option<Box<VoidCallback>>,
    on_close: Option<Box<VoidCallback>>,
    clipboard_context: *mut ClipboardContext,
}
```

Changes:
- `gl_area: gtk::GLArea` → `gl_area: ChosttyTerminalArea`
- Add `scrolled_window: gtk::ScrolledWindow`
- Add `suppress_vadj_signal: Rc<Cell<bool>>`

Ensure `Rc` and `Cell` are in scope (they are — `use std::cell::{Cell, RefCell};` and `use std::rc::Rc;` exist at the top).

- [ ] **Step 4.2: Build to find all broken call sites**

```bash
cd rust/chostty-host-linux && cargo build 2>&1 | head -60
```

Expected: compile errors referring to `entry.gl_area` being `ChosttyTerminalArea` instead of `gtk::GLArea` and a missing-field error at the `SURFACE_MAP.insert(...)` call site. Track each.

- [ ] **Step 4.3: Fix every `entry.gl_area` call site**

`ChosttyTerminalArea` implements `gtk::Accessible`, `gtk::Buildable`, `gtk::ConstraintTarget`, `gtk::Widget`, `gtk::GLArea`, `gtk::Scrollable`. All methods called on `entry.gl_area` that exist on `GLArea` or `Widget` (e.g., `.queue_render()`, `.grab_focus()`) continue to work — no change needed in most cases.

For any call site that explicitly requires a `&gtk::GLArea` reference (e.g., passed by `&gtk::GLArea` to a helper), change the parameter type to `&ChosttyTerminalArea` or upcast via `entry.gl_area.upcast_ref::<gtk::GLArea>()`.

Walk through the compile errors one by one and fix in place. Common errors:

- `queue_render` — works as-is (method inherited from GLArea).
- `add_controller` — works (method from Widget).
- `grab_focus` — works (method from Widget).
- Functions like `request_terminal_focus(gl_area: &gtk::GLArea, ...)` → change the parameter type to `gl_area: &ChosttyTerminalArea` in the function signature (search the file for `fn request_terminal_focus`). There's also `show_terminal_context_menu` which may take a `&gtk::GLArea` — update likewise.

- [ ] **Step 4.4: Fix the `SURFACE_MAP.insert` call site (around line 1145)**

Find the `SurfaceEntry { ... }` construction site (line ~1149). After `gl_area: gl.clone(),` and before `toast_overlay:`, add:

```rust
                        gl_area: gl.clone(),
                        scrolled_window: scrolled_window.clone(),
                        suppress_vadj_signal: suppress_vadj_signal.clone(),
                        toast_overlay: overlay_for_map.clone(),
```

This references `scrolled_window` and `suppress_vadj_signal` locals that **don't exist yet** — they will be created in Task 5. Expect build failure; leave the error for Task 5 to close.

- [ ] **Step 4.5: Skip full build — proceed to Task 5**

The code is intentionally non-building until Task 5 creates `scrolled_window` and `suppress_vadj_signal`. We commit at the end of Task 5.

---

## Task 5: Swap `GLArea::new()` for `ChosttyTerminalArea::new()` and wrap in `ScrolledWindow`

**Files:**
- Modify: `rust/chostty-host-linux/src/terminal.rs`

- [ ] **Step 5.1: Swap the construction**

Find (line ~960):

```rust
    let gl_area = gtk::GLArea::new();
    gl_area.add_css_class("chostty-terminal-glarea");
    gl_area.set_hexpand(true);
    gl_area.set_vexpand(true);
    gl_area.set_auto_render(true);
    gl_area.set_focusable(true);
    gl_area.set_can_focus(true);
    gl_area.connect_map(|gl_area| {
        gl_area.queue_render();
    });
```

Replace the first line with:

```rust
    let gl_area = ChosttyTerminalArea::new();
```

Leave the rest of the block as-is (all those methods exist on the subclass via inheritance).

- [ ] **Step 5.2: Create the `ScrolledWindow` and reentrancy flag before the `Overlay`**

Find (line ~982):

```rust
    // Create overlay early so closures can capture it for toast notifications
    let overlay = gtk::Overlay::new();
    overlay.add_css_class("chostty-terminal-surface");
    overlay.set_child(Some(&gl_area));
```

Replace with:

```rust
    // Read the scrollbar policy from libghostty's config.
    let vscrollbar_policy = {
        let config = load_ghostty_config();
        let policy = scrollbar_policy_from_config(config);
        unsafe { ghostty_config_free(config) };
        policy
    };

    // Wrap the GLArea in a ScrolledWindow so GTK's overlay scrollbar
    // machinery fades the scrollbar in on scroll / mouse activity.
    let scrolled_window = gtk::ScrolledWindow::new();
    scrolled_window.set_hscrollbar_policy(gtk::PolicyType::Never);
    scrolled_window.set_vscrollbar_policy(vscrollbar_policy);
    // The overlay-scrolling property drives the fade-in / thin-indicator /
    // widen-on-hover behavior. It's true by default; set explicitly for
    // clarity.
    scrolled_window.set_overlay_scrolling(true);
    // Terminal scrollback is discrete-row; kinetic scrolling feels wrong
    // (matches upstream Ghostty's workaround).
    scrolled_window.set_kinetic_scrolling(false);
    scrolled_window.set_child(Some(&gl_area));
    scrolled_window.set_hexpand(true);
    scrolled_window.set_vexpand(true);

    // Reentrancy guard: set true while we programmatically update the
    // vadjustment from a SCROLLBAR action so the value-changed handler
    // doesn't turn around and emit scroll_to_row back into libghostty.
    let suppress_vadj_signal = Rc::new(Cell::new(false));

    // Create overlay early so closures can capture it for toast notifications
    let overlay = gtk::Overlay::new();
    overlay.add_css_class("chostty-terminal-surface");
    overlay.set_child(Some(&scrolled_window));
```

Key change: `overlay.set_child(Some(&scrolled_window))` — the overlay's direct child is now the ScrolledWindow, which in turn holds the GLArea.

- [ ] **Step 5.3: Remove the now-unused `#[allow(dead_code)]`**

On `scrollbar_policy_from_tag` and `scrollbar_policy_from_config` (added in Task 2), drop the `#[allow(dead_code)]` attributes — both are now called.

- [ ] **Step 5.4: Build**

```bash
cd rust/chostty-host-linux && cargo build 2>&1 | tail -40
```

Fix any remaining compile errors. Common issues:
- `gl_area.clone()` call sites where closures capture `gl_area` — `ChosttyTerminalArea` derives `Clone` via `glib::wrapper!`, so `.clone()` still works (it's a ref-count bump, not a deep clone).
- If a closure captures `&gtk::GLArea` reference, the compiler may ask for explicit `.upcast_ref::<gtk::GLArea>()`. Add that as needed.
- If any helper function signature in `terminal.rs` takes `&gtk::GLArea`, change to `&ChosttyTerminalArea`. Examples from grep: `request_terminal_focus`, `show_terminal_context_menu`, possibly others. Search: `rg "&gtk::GLArea" rust/chostty-host-linux/src/terminal.rs`.

- [ ] **Step 5.5: Run the full quality gate**

```bash
./scripts/check.sh
```
Expected: PASS.

- [ ] **Step 5.6: Smoke-test manually**

Build and launch:
```bash
cargo build --release
LD_LIBRARY_PATH=./ghostty/zig-out/lib:$LD_LIBRARY_PATH ./target/release/chostty
```

Expected: Chostty starts, opens a workspace, terminal renders normally (no scrollbar behavior yet — the adjustment isn't wired — but rendering, typing, resizing, and `Ctrl+scroll` for font size all work). If the terminal is blank or crashes, investigate before continuing.

- [ ] **Step 5.7: Commit**

```bash
git add rust/chostty-host-linux/src/terminal.rs
git commit -m "$(cat <<'EOF'
Wrap terminal GLArea in a ScrolledWindow

Use the new ChosttyTerminalArea subclass and wrap it in a
GtkScrolledWindow inside the existing per-terminal GtkOverlay.
Read the scrollbar policy from libghostty's config at construction
(system → automatic overlay scrollbar, never → no scrollbar).
kinetic_scrolling is disabled because terminal scrollback is
discrete-row.

SurfaceEntry now holds a ref to the ScrolledWindow and a
reentrancy flag used by the forthcoming SCROLLBAR action handler.
No adjustment wiring yet — the scrollbar stays inactive until the
next task.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: Handle `GHOSTTY_ACTION_SCROLLBAR` — drive the vadjustment from core

**Files:**
- Modify: `rust/chostty-host-linux/src/terminal.rs`

- [ ] **Step 6.1: Add the `apply_scrollbar` helper**

In `rust/chostty-host-linux/src/terminal.rs`, directly above `unsafe extern "C" fn ghostty_action_cb` (line ~457), add:

```rust
fn apply_scrollbar_to_entry(entry: &SurfaceEntry, s: ghostty_action_scrollbar_s) {
    use gtk::prelude::*;
    let vadj = match gtk::prelude::ScrollableExt::vadjustment(&entry.gl_area) {
        Some(a) => a,
        None => return,
    };

    let value = s.offset as f64;
    let upper = s.total as f64;
    let page_size = s.len as f64;

    // Skip updates that wouldn't change the adjustment (upstream does the
    // same; every pty redraw emits a SCROLLBAR action even if unchanged).
    if (vadj.value() - value).abs() < 0.001
        && (vadj.upper() - upper).abs() < 0.001
        && (vadj.page_size() - page_size).abs() < 0.001
    {
        return;
    }

    entry.suppress_vadj_signal.set(true);
    vadj.configure(value, 0.0, upper, 1.0, page_size, page_size);
    entry.suppress_vadj_signal.set(false);
}
```

Ensure `ghostty_action_scrollbar_s` is in the existing `use chostty_ghostty_sys::{...};` at the top of the file. Also add `GHOSTTY_ACTION_SCROLLBAR` to the same import list.

- [ ] **Step 6.2: Add the `GHOSTTY_ACTION_SCROLLBAR` arm to the action callback**

In `ghostty_action_cb` (line ~457), find the existing `GHOSTTY_ACTION_RENDER` arm (line ~465) — it's a clean example of a surface-targeted action. Immediately **after** the `GHOSTTY_ACTION_RENDER` arm, add:

```rust
        GHOSTTY_ACTION_SCROLLBAR => {
            if target.tag == GHOSTTY_TARGET_SURFACE {
                let surface_key = unsafe { target.target.surface } as usize;
                let s = unsafe { action.action.scrollbar };
                SURFACE_MAP.with(|map| {
                    if let Some(entry) = map.borrow().get(&surface_key) {
                        apply_scrollbar_to_entry(entry, s);
                    }
                });
            }
            true
        }
```

- [ ] **Step 6.3: Build**

```bash
cd rust/chostty-host-linux && cargo build
```
Expected: clean compile.

- [ ] **Step 6.4: Smoke-test manually**

```bash
cargo build --release
LD_LIBRARY_PATH=./ghostty/zig-out/lib:$LD_LIBRARY_PATH ./target/release/chostty
```

In the terminal, run `seq 10000` to generate scrollback. Then wheel-scroll up. Expected: the overlay scrollbar fades in on the right edge as a thin indicator. Hovering the handle: track appears and handle widens. Dragging the handle: **no effect yet** (drag handler not wired until Task 7).

If the scrollbar does not appear:
- Confirm `scrollbar = system` in your ghostty config (or not set — the default is `system`).
- Check that the `ChosttyTerminalArea` is actually inside the `ScrolledWindow`: using `GTK_DEBUG=interactive` launch flag to inspect the widget tree.
- Confirm the `SCROLLBAR` action is firing: add a `tracing::debug!` at the top of the arm and check logs.

- [ ] **Step 6.5: Commit**

```bash
git add rust/chostty-host-linux/src/terminal.rs
git commit -m "$(cat <<'EOF'
Wire GHOSTTY_ACTION_SCROLLBAR to the vadjustment

Apply {total, offset, len} from libghostty's scrollbar action to
the terminal's vertical GtkAdjustment. A short-circuit avoids
spurious configure() calls for unchanged values. Drag-to-scroll
(UI → core) is not wired yet — that's the next task.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: Drive core from UI — wire vadjustment `value-changed` → `scroll_to_row`

**Files:**
- Modify: `rust/chostty-host-linux/src/terminal.rs`

- [ ] **Step 7.1: Connect the value-changed handler after the ScrolledWindow is set up**

Find the block (just added in Task 5) that creates the `ScrolledWindow`. **After** the `suppress_vadj_signal` line and before the `let overlay = gtk::Overlay::new();` line, add:

```rust
    // Wire the vadjustment's value-changed signal so user drags on the
    // scrollbar handle translate to scroll_to_row:<n> binding actions.
    // Skip the action when we're inside a programmatic SCROLLBAR update
    // (guarded by suppress_vadj_signal).
    {
        let vadj = scrolled_window.vadjustment();
        let surface_cell = surface_cell.clone();
        let suppress = suppress_vadj_signal.clone();
        vadj.connect_value_changed(move |adj| {
            if suppress.get() {
                return;
            }
            let surface = match *surface_cell.borrow() {
                Some(s) => s,
                None => return,
            };
            let row = adj.value().round() as usize;
            let action = format!("scroll_to_row:{row}");
            unsafe {
                ghostty_surface_binding_action(
                    surface,
                    action.as_ptr() as *const c_char,
                    action.len(),
                );
            }
        });
    }
```

Ensure `ghostty_surface_binding_action` is in the existing FFI import list. `c_char` should also already be imported.

- [ ] **Step 7.2: Build**

```bash
cd rust/chostty-host-linux && cargo build
```
Expected: clean compile.

- [ ] **Step 7.3: Smoke-test manually**

```bash
cargo build --release
LD_LIBRARY_PATH=./ghostty/zig-out/lib:$LD_LIBRARY_PATH ./target/release/chostty
```

Steps in the running app:
1. `seq 10000` to fill scrollback.
2. Wheel up: scrollbar fades in.
3. Drag the scrollbar handle up: terminal viewport should jump to the dragged position. Release: position persists.
4. Drag down: returns to bottom; scrollbar fades out when scrolled to bottom.
5. Wheel up, then `Ctrl+scroll` up: font size should increase (modifier-aware scroll is not intercepted by the ScrolledWindow).

If dragging the handle has no effect:
- Log inside the `connect_value_changed` closure to confirm it fires on drag.
- Confirm the `scroll_to_row:<n>` binding string format is correct (upstream Zig: `.scroll_to_row = row` — the C API accepts the same string `scroll_to_row:<usize>`).
- Confirm `ghostty_surface_binding_action` returns `true` (or at least doesn't panic).

If drag behavior feels janky (e.g., repeated re-snapping):
- Check that `suppress_vadj_signal` is correctly bracketing the `configure` call in Task 6's helper.

- [ ] **Step 7.4: Commit**

```bash
git add rust/chostty-host-linux/src/terminal.rs
git commit -m "$(cat <<'EOF'
Drive core scroll position from scrollbar drags

Connect the vadjustment's value-changed signal to emit
scroll_to_row:<n> via ghostty_surface_binding_action. Skip the
action when we're inside a programmatic update (the SCROLLBAR
action path sets a reentrancy flag).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 8: Apply the scrollbar policy on live config reload

**Files:**
- Modify: `rust/chostty-host-linux/src/terminal.rs`

- [ ] **Step 8.1: Extend the `GHOSTTY_ACTION_RELOAD_CONFIG` arm**

Find (line ~575):

```rust
        GHOSTTY_ACTION_RELOAD_CONFIG => {
            let config = load_ghostty_config();
            match target.tag {
                GHOSTTY_TARGET_APP => unsafe {
                    ghostty_app_update_config(app, config);
                },
                GHOSTTY_TARGET_SURFACE => {
                    let surface = unsafe { target.target.surface };
                    unsafe {
                        ghostty_surface_update_config(surface, config);
                    }
                }
                _ => {}
            }
            unsafe {
                ghostty_config_free(config);
            }
            true
        }
```

Replace the `GHOSTTY_TARGET_SURFACE` branch body (the three-line block) with:

```rust
                GHOSTTY_TARGET_SURFACE => {
                    let surface = unsafe { target.target.surface };
                    unsafe {
                        ghostty_surface_update_config(surface, config);
                    }
                    // Re-apply the scrollbar policy in case it changed.
                    let policy = scrollbar_policy_from_config(config);
                    let surface_key = surface as usize;
                    SURFACE_MAP.with(|map| {
                        if let Some(entry) = map.borrow().get(&surface_key) {
                            entry.scrolled_window.set_vscrollbar_policy(policy);
                        }
                    });
                }
```

- [ ] **Step 8.2: Build**

```bash
cd rust/chostty-host-linux && cargo build
```
Expected: clean compile.

- [ ] **Step 8.3: Smoke-test manually**

```bash
cargo build --release
LD_LIBRARY_PATH=./ghostty/zig-out/lib:$LD_LIBRARY_PATH ./target/release/chostty
```

1. With default config, verify scrollbar appears on scroll.
2. Edit `~/.config/ghostty/config`, add line `scrollbar = never`.
3. Trigger a config reload if a keybind for it exists (check `window.rs` for `ReloadConfig` action), otherwise restart Chostty.
4. Expected: scrollbar no longer appears, wheel still works for scrollback.
5. Revert config.

- [ ] **Step 8.4: Commit**

```bash
git add rust/chostty-host-linux/src/terminal.rs
git commit -m "$(cat <<'EOF'
Re-apply scrollbar policy on config reload

When GHOSTTY_ACTION_RELOAD_CONFIG fires for a surface, re-read the
`scrollbar` key and update the ScrolledWindow's vertical policy so
toggling `scrollbar = system | never` takes effect without a restart.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 9: Full quality gate + manual verification

**Files:**
- None (verification only).

- [ ] **Step 9.1: Run the full quality gate**

```bash
./scripts/check.sh
```
Expected: PASS (fmt, clippy with `-D warnings`, all workspace tests).

- [ ] **Step 9.2: Run the manual verification checklist**

Build release and launch:

```bash
cargo build --release
LD_LIBRARY_PATH=./ghostty/zig-out/lib:$LD_LIBRARY_PATH ./target/release/chostty
```

Verify each item:

- [ ] **(a)** Open a terminal pane.
- [ ] **(b)** Run `seq 10000`. Scrollback fills.
- [ ] **(c)** Wheel-scroll up: a thin, semi-transparent indicator fades in on the right edge.
- [ ] **(d)** Hover the indicator/handle: a track appears and the handle widens.
- [ ] **(e)** Drag the handle: the terminal viewport moves to that scrollback row; release holds the position.
- [ ] **(f)** Scroll back to bottom: the indicator fades out after a moment of inactivity.
- [ ] **(g)** `Ctrl+D` to split right: each pane has its own independent scrollbar.
- [ ] **(h)** `Ctrl+scroll` in either pane: font size still changes (wheel events are not eaten by the ScrolledWindow).
- [ ] **(i)** Close a pane (`Ctrl+W`) while scrolled back: no crash, no stray widgets, neighbor pane still works.
- [ ] **(j)** Set `scrollbar = never` in `~/.config/ghostty/config`, relaunch Chostty: no scrollbar appears at any time; wheel still scrolls scrollback.
- [ ] **(k)** Set `scrollbar = system` again, relaunch: scrollbar behavior is restored.

If any of these fail, fix before claiming done. If the bug is subtle, consider using `superpowers:systematic-debugging`.

- [ ] **Step 9.3: (Optional) Remove any temporary `#[allow(dead_code)]` that are no longer needed**

Double-check: `scrollbar_policy_from_tag` and `scrollbar_policy_from_config` are both called in the production path now. The `#[allow(dead_code)]` on them (from Task 2) should already have been removed in Task 5.3. If any are still present, remove them now and re-run `./scripts/check.sh`.

- [ ] **Step 9.4: Commit any cleanup**

If there was cleanup in 9.3:
```bash
git add rust/chostty-host-linux/src/terminal.rs
git commit -m "$(cat <<'EOF'
Remove temporary dead_code attributes

All the helpers introduced for the scrollbar feature are now on the
hot path.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Done

The branch `ghostty-scroll-bars` should now contain a full working implementation of GTK-overlay-style scrollbars for every Chostty terminal pane, respecting the user's Ghostty config and live-reloading when the config changes.

Follow up with `superpowers:finishing-a-development-branch` to decide merge/PR/cleanup.
