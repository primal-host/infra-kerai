use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::home;

#[derive(Debug, Deserialize, Default)]
pub struct ConfigFile {
    pub default: Option<Profile>,
    pub profiles: Option<HashMap<String, Profile>>,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct Profile {
    pub connection: Option<String>,
    pub crate_name: Option<String>,
}

impl Profile {
    /// Merge another profile into this one (other takes priority for set fields).
    pub fn merge(&mut self, other: &Profile) {
        if other.connection.is_some() {
            self.connection = other.connection.clone();
        }
        if other.crate_name.is_some() {
            self.crate_name = other.crate_name.clone();
        }
    }
}

/// Walk up from the current directory looking for `.kerai/config.toml`.
pub fn find_project_config() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        let candidate = dir.join(".kerai").join("config.toml");
        if candidate.is_file() {
            return Some(candidate);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Return the project root (parent of `.kerai/`) if a project config exists.
pub fn find_project_root() -> Option<PathBuf> {
    find_project_config().map(|p| p.parent().unwrap().parent().unwrap().to_path_buf())
}

/// Global config path: `~/.config/kerai/config.toml`.
pub fn global_config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("kerai").join("config.toml"))
}

fn load_file(path: &Path) -> Option<ConfigFile> {
    let content = std::fs::read_to_string(path).ok()?;
    toml::from_str(&content).ok()
}

/// Load connection config from `~/.kerai/kerai.kerai`.
fn load_kerai_config(kerai_home: &Path) -> Option<Profile> {
    let map = home::load_kerai_file(kerai_home).ok()?;
    let connection = map.get("postgres.global.connection").cloned();
    if connection.is_some() {
        Some(Profile {
            connection,
            ..Default::default()
        })
    } else {
        None
    }
}

/// Resolve a profile by name, merging:
/// Global TOML default → Global TOML named → kerai.kerai → Project TOML default → Project TOML named
pub fn load_config(profile_name: &str) -> Profile {
    let mut result = Profile::default();

    // Global TOML config
    if let Some(path) = global_config_path() {
        if let Some(cfg) = load_file(&path) {
            if let Some(default) = &cfg.default {
                result.merge(default);
            }
            if profile_name != "default" {
                if let Some(profiles) = &cfg.profiles {
                    if let Some(named) = profiles.get(profile_name) {
                        result.merge(named);
                    }
                }
            }
        }
    }

    // kerai.kerai (global kerai-native config, between global and project TOML)
    if let Ok(kerai_home) = home::ensure_home_dir() {
        if let Some(kerai_profile) = load_kerai_config(&kerai_home) {
            result.merge(&kerai_profile);
        }
    }

    // Project TOML config (highest priority)
    if let Some(path) = find_project_config() {
        if let Some(cfg) = load_file(&path) {
            if let Some(default) = &cfg.default {
                result.merge(default);
            }
            if profile_name != "default" {
                if let Some(profiles) = &cfg.profiles {
                    if let Some(named) = profiles.get(profile_name) {
                        result.merge(named);
                    }
                }
            }
        }
    }

    result
}
