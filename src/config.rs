use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use eyre::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::hotkey::Hotkeys;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ViewMode {
    #[default]
    Month,
    Week,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default = "default_calendar_dir")]
    pub calendar_dir: PathBuf,
    #[serde(default = "default_calendar")]
    pub default_calendar: String,
    #[serde(default)]
    pub default_view: ViewMode,
    #[serde(default = "default_start_time")]
    pub default_start_time: String,
    #[serde(default = "default_duration")]
    pub default_duration_minutes: i64,
    #[serde(default)]
    pub calendar_colors: BTreeMap<String, String>,
    #[serde(default)]
    pub hotkeys: Hotkeys,
}

fn default_calendar_dir() -> PathBuf {
    PathBuf::from("~/.local/share/dav/calendar")
}

fn default_calendar() -> String {
    "default".to_string()
}

fn default_start_time() -> String {
    "09:00".to_string()
}

fn default_duration() -> i64 {
    60
}

impl Default for Config {
    fn default() -> Self {
        Self {
            calendar_dir: default_calendar_dir(),
            default_calendar: default_calendar(),
            default_view: ViewMode::Month,
            default_start_time: default_start_time(),
            default_duration_minutes: default_duration(),
            calendar_colors: BTreeMap::new(),
            hotkeys: Hotkeys::default(),
        }
    }
}

impl Config {
    pub fn default_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("calendar/config.yml")
    }

    pub fn load_or_create(path: &Path) -> Result<Self> {
        if !path.exists() {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .wrap_err_with(|| format!("creating {}", parent.display()))?;
            }
            fs::write(path, include_str!("../assets/default_config.yml"))
                .wrap_err_with(|| format!("writing default config to {}", path.display()))?;
        }
        let input = fs::read_to_string(path)
            .wrap_err_with(|| format!("reading config {}", path.display()))?;
        let mut config: Self = serde_yaml::from_str(&input)
            .wrap_err_with(|| format!("parsing config {}", path.display()))?;
        config.hotkeys.migrate_new_bindings();
        Ok(config)
    }

    pub fn expand_home(path: &Path) -> PathBuf {
        let Some(path_str) = path.to_str() else {
            return path.to_path_buf();
        };
        if path_str == "~" {
            return dirs::home_dir().unwrap_or_else(|| path.to_path_buf());
        }
        if let Some(rest) = path_str.strip_prefix("~/")
            && let Some(home) = dirs::home_dir()
        {
            return home.join(rest);
        }
        path.to_path_buf()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_config_deserializes() {
        let config: Config = serde_yaml::from_str(include_str!("../assets/default_config.yml"))
            .expect("bundled config should be usable");
        assert_eq!(config.default_view, ViewMode::Month);
        assert_eq!(config.hotkeys.normal.rows().len(), 18);
        assert_eq!(config.hotkeys.agenda.rows().len(), 7);
        assert_eq!(config.hotkeys.confirm.rows().len(), 2);
    }

    #[test]
    fn missing_config_is_created_with_all_hotkeys() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("nested/config.yml");
        let config = Config::load_or_create(&path).unwrap();
        assert!(path.is_file());
        assert!(
            config
                .hotkeys
                .normal
                .key_for(&crate::hotkey::NormalAction::AddEvent)
                .is_some()
        );
        assert!(
            config
                .hotkeys
                .normal
                .key_for(&crate::hotkey::NormalAction::OpenAgenda)
                .is_some()
        );
        assert!(
            config
                .hotkeys
                .agenda
                .key_for(&crate::hotkey::AgendaAction::Edit)
                .is_some()
        );
        assert!(
            config
                .hotkeys
                .dialog
                .key_for(&crate::hotkey::DialogAction::Save)
                .is_some()
        );
        assert!(
            config
                .hotkeys
                .dialog
                .key_for(&crate::hotkey::DialogAction::ToggleMode)
                .is_some()
        );
    }

    #[test]
    fn existing_config_gets_the_new_mode_toggle_binding() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.yml");
        let old_config =
            include_str!("../assets/default_config.yml").replace("    \"C-t\": ToggleMode\n", "");
        fs::write(&path, old_config).unwrap();

        let config = Config::load_or_create(&path).unwrap();

        assert_eq!(
            config
                .hotkeys
                .dialog
                .key_for(&crate::hotkey::DialogAction::ToggleMode)
                .as_deref(),
            Some("C-t")
        );
    }

    #[test]
    fn existing_config_gets_agenda_arrow_bindings() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.yml");
        let old_config = include_str!("../assets/default_config.yml")
            .replace("    \"Down\": !Navigate Down\n", "")
            .replace("    \"Up\": !Navigate Up\n", "");
        fs::write(&path, old_config).unwrap();

        let config = Config::load_or_create(&path).unwrap();

        assert!(config.hotkeys.agenda.bindings.iter().any(|(action, keys)| {
            action == &crate::hotkey::AgendaAction::Navigate(crate::hotkey::Direction::Down)
                && keys.display() == "Down"
        }));
        assert!(config.hotkeys.agenda.bindings.iter().any(|(action, keys)| {
            action == &crate::hotkey::AgendaAction::Navigate(crate::hotkey::Direction::Up)
                && keys.display() == "Up"
        }));
    }
}
