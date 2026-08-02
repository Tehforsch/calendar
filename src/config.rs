use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use eyre::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::hotkey::Hotkeys;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ViewMode {
    Month,
    Week,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub calendar_dir: PathBuf,
    pub default_calendar: String,
    pub default_view: ViewMode,
    pub default_start_time: String,
    pub default_duration_minutes: i64,
    pub calendar_colors: BTreeMap<String, String>,
    pub hotkeys: Hotkeys,
}

impl Default for Config {
    fn default() -> Self {
        serde_yaml::from_str(include_str!("../assets/default_config.yml"))
            .expect("template config must be valid")
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
        serde_yaml::from_str(&input).wrap_err_with(|| format!("parsing config {}", path.display()))
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
        assert_eq!(config.hotkeys.normal.rows().len(), 24);
        assert_eq!(config.hotkeys.dialog.rows().len(), 7);
        assert_eq!(config.hotkeys.agenda.rows().len(), 17);
        assert_eq!(config.hotkeys.confirm.rows().len(), 3);
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
}
