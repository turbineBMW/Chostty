use std::path::PathBuf;
use std::process::Command;

use crate::app_config::NotificationSoundConfig;

const PRIMARY_THEME_EVENT_ID: &str = "complete";
const FALLBACK_THEME_EVENT_ID: &str = "message";

pub fn should_play_for_attention_transition(
    is_target_visible: bool,
    already_notified: bool,
) -> bool {
    !is_target_visible && !already_notified
}

pub fn play(sound: &NotificationSoundConfig) {
    if !sound.enabled {
        return;
    }

    if let Some(path) = sound_file_path(sound) {
        if spawn_canberra_play_file(&path).is_ok() {
            return;
        }
    }

    if spawn_canberra_play_id(PRIMARY_THEME_EVENT_ID).is_err() {
        let _ = spawn_canberra_play_id(FALLBACK_THEME_EVENT_ID);
    }
}

fn sound_file_path(sound: &NotificationSoundConfig) -> Option<PathBuf> {
    sound
        .custom_file
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_file())
}

fn spawn_canberra_play_file(path: &PathBuf) -> Result<(), String> {
    Command::new("canberra-gtk-play")
        .arg("--file")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|err| format!("failed to play custom notification sound: {err}"))
}

fn spawn_canberra_play_id(event_id: &str) -> Result<(), String> {
    Command::new("canberra-gtk-play")
        .arg("--id")
        .arg(event_id)
        .spawn()
        .map(|_| ())
        .map_err(|err| {
            eprintln!("chostty: failed to play notification sound `{event_id}`: {err}");
            format!("failed to play notification sound `{event_id}`: {err}")
        })
}

#[cfg(test)]
mod tests {
    use super::should_play_for_attention_transition;

    #[test]
    fn plays_only_for_first_attention_transition_of_hidden_target() {
        assert!(should_play_for_attention_transition(false, false));
        assert!(!should_play_for_attention_transition(true, false));
        assert!(!should_play_for_attention_transition(false, true));
        assert!(!should_play_for_attention_transition(true, true));
    }
}
