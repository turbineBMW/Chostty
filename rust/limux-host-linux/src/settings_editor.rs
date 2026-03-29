use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use adw::prelude::*;
use gtk4 as gtk;
use libadwaita as adw;

use crate::app_config::{self, AppConfig, ColorScheme};
use crate::keybind_editor;
use crate::shortcut_config::{NormalizedShortcut, ResolvedShortcutConfig, ShortcutId};

pub const SETTINGS_CSS: &str = r#"
.limux-settings-window {
    background-color: @window_bg_color;
    color: @window_fg_color;
}
"#;

type OnConfigChanged = dyn Fn(&AppConfig, &AppConfig);

pub struct SettingsEditorInput {
    pub config: Rc<RefCell<AppConfig>>,
    pub shortcuts: Rc<ResolvedShortcutConfig>,
    pub on_capture: Rc<
        dyn Fn(ShortcutId, Option<NormalizedShortcut>) -> Result<ResolvedShortcutConfig, String>,
    >,
    pub on_config_changed: Rc<OnConfigChanged>,
}

pub fn present_settings_dialog(parent: &impl IsA<gtk::Widget>, input: SettingsEditorInput) {
    let window = adw::Window::new();
    window.set_title(Some("Settings"));
    window.set_default_size(760, 680);
    window.set_modal(true);

    if let Some(parent_window) = parent
        .root()
        .and_then(|root| root.downcast::<gtk::Window>().ok())
    {
        window.set_transient_for(Some(&parent_window));
        if let Some(app) = parent_window.application() {
            window.set_application(Some(&app));
        }
    }

    let content = build_settings_window_content(&window, input);
    window.set_content(Some(&content));
    window.present();
}

fn apply_config_change<F, G>(config: &Rc<RefCell<AppConfig>>, on_changed: &F, update: G)
where
    F: Fn(&AppConfig, &AppConfig) + ?Sized,
    G: FnOnce(&mut AppConfig),
{
    let (previous, updated) = {
        let mut config_ref = config.borrow_mut();
        let previous = config_ref.clone();
        update(&mut config_ref);
        let updated = config_ref.clone();
        (previous, updated)
    };
    on_changed(&previous, &updated);
}

fn build_settings_window_content(window: &adw::Window, input: SettingsEditorInput) -> gtk::Widget {
    let stack = adw::ViewStack::new();
    stack.set_hexpand(true);
    stack.set_vexpand(true);

    let general_page = build_general_page(&input);
    let general_stack_page = stack.add_titled(&general_page, Some("general"), "General");
    general_stack_page.set_icon_name(Some("preferences-system-symbolic"));

    let keybinds_page = keybind_editor::build_keybind_editor(&input.shortcuts, input.on_capture);
    let keybinds_stack_page = stack.add_titled(&keybinds_page, Some("keybindings"), "Keybindings");
    keybinds_stack_page.set_icon_name(Some("input-keyboard-symbolic"));

    let switcher = adw::ViewSwitcher::builder()
        .stack(&stack)
        .policy(adw::ViewSwitcherPolicy::Wide)
        .build();

    let close_button = gtk::Button::builder()
        .icon_name("window-close-symbolic")
        .tooltip_text("Close settings")
        .valign(gtk::Align::Center)
        .build();
    close_button.add_css_class("flat");

    {
        let window = window.clone();
        close_button.connect_clicked(move |_| {
            window.close();
        });
    }

    let header_bar = adw::HeaderBar::new();
    header_bar.set_show_start_title_buttons(false);
    header_bar.set_show_end_title_buttons(false);
    header_bar.set_title_widget(Some(&switcher));
    header_bar.pack_end(&close_button);

    let outer = gtk::Box::new(gtk::Orientation::Vertical, 0);
    outer.add_css_class("limux-settings-window");
    outer.append(&header_bar);
    outer.append(&stack);
    outer.upcast()
}

fn build_general_page(input: &SettingsEditorInput) -> gtk::Widget {
    let page = adw::PreferencesPage::new();
    page.set_title("General");
    page.set_name(Some("general"));
    page.set_icon_name(Some("preferences-system-symbolic"));
    page.set_hexpand(true);
    page.set_vexpand(true);

    let group = adw::PreferencesGroup::new();

    let color_row = adw::ActionRow::builder()
        .title("GTK color scheme")
        .subtitle("Choose whether the GTK interface follows system, dark, or light")
        .build();
    color_row.set_title_lines(1);
    color_row.set_subtitle_lines(2);
    let color_dropdown = gtk::DropDown::from_strings(&["System", "Dark", "Light"]);
    let initial_scheme = input.config.borrow().appearance.color_scheme;
    color_dropdown.set_selected(match initial_scheme {
        ColorScheme::System => 0,
        ColorScheme::Dark => 1,
        ColorScheme::Light => 2,
    });
    color_dropdown.set_valign(gtk::Align::Center);
    color_row.add_suffix(&color_dropdown);
    color_row.set_activatable_widget(Some(&color_dropdown));
    group.add(&color_row);

    let ghostty_row = adw::ActionRow::builder()
        .title("Ghostty color scheme")
        .subtitle("Choose whether terminal surfaces follow system, dark, or light")
        .build();
    ghostty_row.set_title_lines(1);
    ghostty_row.set_subtitle_lines(2);
    let ghostty_dropdown = gtk::DropDown::from_strings(&["System", "Dark", "Light"]);
    let initial_ghostty_scheme = input.config.borrow().appearance.ghostty_color_scheme;
    ghostty_dropdown.set_selected(match initial_ghostty_scheme {
        ColorScheme::System => 0,
        ColorScheme::Dark => 1,
        ColorScheme::Light => 2,
    });
    ghostty_dropdown.set_valign(gtk::Align::Center);
    ghostty_row.add_suffix(&ghostty_dropdown);
    ghostty_row.set_activatable_widget(Some(&ghostty_dropdown));
    group.add(&ghostty_row);

    let hover_row = adw::ActionRow::builder()
        .title("Hover terminal focus")
        .subtitle("Focus terminal panes when the mouse pointer enters them")
        .build();
    hover_row.set_title_lines(1);
    hover_row.set_subtitle_lines(2);
    let hover_switch = gtk::Switch::new();
    hover_switch.set_active(input.config.borrow().focus.hover_terminal_focus);
    hover_switch.set_valign(gtk::Align::Center);
    hover_row.add_suffix(&hover_switch);
    hover_row.set_activatable_widget(Some(&hover_switch));
    group.add(&hover_row);

    let workspace_row_subtitle =
        "Folder chooser and Open by Path start here. Leave empty to use your home directory.";
    let workspace_row = adw::ActionRow::builder()
        .title("Default workspace directory")
        .subtitle(workspace_row_subtitle)
        .build();
    workspace_row.set_title_lines(1);
    workspace_row.set_subtitle_lines(3);
    let workspace_entry = gtk::Entry::builder().hexpand(true).width_chars(30).build();
    workspace_entry.set_valign(gtk::Align::Center);
    workspace_entry.set_text(
        input
            .config
            .borrow()
            .workspace
            .default_directory
            .as_deref()
            .unwrap_or(""),
    );
    if let Some(default_dir) =
        app_config::effective_workspace_default_directory(&input.config.borrow())
    {
        workspace_entry.set_placeholder_text(Some(default_dir.to_string_lossy().as_ref()));
    }
    let workspace_clear_button = gtk::Button::builder()
        .icon_name("edit-clear-symbolic")
        .tooltip_text("Use home directory")
        .valign(gtk::Align::Center)
        .build();
    workspace_clear_button.add_css_class("flat");
    workspace_row.add_suffix(&workspace_entry);
    workspace_row.add_suffix(&workspace_clear_button);
    workspace_row.set_activatable_widget(Some(&workspace_entry));
    group.add(&workspace_row);

    page.add(&group);

    {
        let config = input.config.clone();
        let on_changed = input.on_config_changed.clone();
        color_dropdown.connect_selected_notify(move |dropdown| {
            let scheme = match dropdown.selected() {
                1 => ColorScheme::Dark,
                2 => ColorScheme::Light,
                _ => ColorScheme::System,
            };
            apply_config_change(&config, &*on_changed, move |c| {
                c.appearance.color_scheme = scheme;
            });
        });
    }
    {
        let config = input.config.clone();
        let on_changed = input.on_config_changed.clone();
        ghostty_dropdown.connect_selected_notify(move |dropdown| {
            let scheme = match dropdown.selected() {
                1 => ColorScheme::Dark,
                2 => ColorScheme::Light,
                _ => ColorScheme::System,
            };
            apply_config_change(&config, &*on_changed, move |c| {
                c.appearance.ghostty_color_scheme = scheme;
            });
        });
    }
    {
        let config = input.config.clone();
        let on_changed = input.on_config_changed.clone();
        hover_switch.connect_active_notify(move |switch| {
            let hover_terminal_focus = switch.is_active();
            apply_config_change(&config, &*on_changed, move |c| {
                c.focus.hover_terminal_focus = hover_terminal_focus;
            });
        });
    }
    {
        let config = input.config.clone();
        let on_changed = input.on_config_changed.clone();
        let row = workspace_row.clone();
        let entry = workspace_entry.clone();
        workspace_entry.connect_activate(move |_| {
            commit_workspace_default_directory(
                &config,
                &*on_changed,
                &row,
                &entry,
                workspace_row_subtitle,
            );
        });
    }
    {
        let config = input.config.clone();
        let on_changed = input.on_config_changed.clone();
        let row = workspace_row.clone();
        let entry = workspace_entry.clone();
        let focus = gtk::EventControllerFocus::new();
        focus.connect_leave(move |_| {
            commit_workspace_default_directory(
                &config,
                &*on_changed,
                &row,
                &entry,
                workspace_row_subtitle,
            );
        });
        workspace_entry.add_controller(focus);
    }
    {
        let config = input.config.clone();
        let on_changed = input.on_config_changed.clone();
        let row = workspace_row.clone();
        let entry = workspace_entry.clone();
        workspace_clear_button.connect_clicked(move |_| {
            row.set_subtitle(workspace_row_subtitle);
            entry.set_text("");
            let already_unset = config.borrow().workspace.default_directory.is_none();
            if already_unset {
                return;
            }
            apply_config_change(&config, &*on_changed, |c| {
                c.workspace.default_directory = None;
            });
        });
    }

    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .child(&page)
        .build();
    scroller.set_hexpand(true);
    scroller.set_vexpand(true);

    scroller.upcast()
}

fn commit_workspace_default_directory<F>(
    config: &Rc<RefCell<AppConfig>>,
    on_changed: &F,
    row: &adw::ActionRow,
    entry: &gtk::Entry,
    default_subtitle: &str,
) where
    F: Fn(&AppConfig, &AppConfig) + ?Sized,
{
    match normalize_workspace_default_directory_input(entry.text().as_str()) {
        Ok(default_directory) => {
            row.set_subtitle(default_subtitle);
            let current = config.borrow().workspace.default_directory.clone();
            if current == default_directory {
                if let Some(value) = default_directory.as_deref() {
                    entry.set_text(value);
                }
                return;
            }
            if let Some(value) = default_directory.as_deref() {
                entry.set_text(value);
            } else {
                entry.set_text("");
            }
            apply_config_change(config, on_changed, move |c| {
                c.workspace.default_directory = default_directory.clone();
            });
        }
        Err(message) => {
            row.set_subtitle(&message);
        }
    }
}

fn normalize_workspace_default_directory_input(input: &str) -> Result<Option<String>, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let path = expand_workspace_setting_path(trimmed)
        .ok_or_else(|| "Home directory is unavailable on this system".to_string())?;
    if !path.exists() {
        return Err("Directory does not exist".to_string());
    }
    if !path.is_dir() {
        return Err("Path is not a directory".to_string());
    }

    Ok(Some(path.to_string_lossy().to_string()))
}

fn expand_workspace_setting_path(input: &str) -> Option<PathBuf> {
    if input == "~" {
        return dirs::home_dir();
    }
    if let Some(rest) = input.strip_prefix("~/") {
        return dirs::home_dir().map(|home| home.join(rest));
    }

    let path = PathBuf::from(input);
    if path.is_absolute() {
        Some(path)
    } else {
        dirs::home_dir().map(|home| home.join(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_config_change_allows_reentrant_config_sync() {
        let config = Rc::new(RefCell::new(AppConfig::default()));

        apply_config_change(
            &config,
            &|_previous, updated| {
                config.borrow_mut().clone_from(updated);
            },
            |current| {
                current.focus.hover_terminal_focus = true;
            },
        );

        assert!(config.borrow().focus.hover_terminal_focus);
    }

    #[test]
    fn normalize_workspace_default_directory_input_expands_home_relative_paths() {
        let home = dirs::home_dir().expect("home dir");
        let input = format!(
            "~/{}",
            home.file_name().unwrap_or_default().to_string_lossy()
        );

        let resolved = expand_workspace_setting_path(&input).expect("expand path");

        assert!(resolved.starts_with(&home));
    }

    #[test]
    fn normalize_workspace_default_directory_input_accepts_empty_value() {
        assert_eq!(
            normalize_workspace_default_directory_input("   ").unwrap(),
            None
        );
    }
}
