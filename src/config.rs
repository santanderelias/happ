use crate::app::SortBy;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct AppConfig {
    pub show_hidden_files: bool,
    pub sort_by: SortBy,
    pub sort_ascending: bool,
    pub history: Vec<PathBuf>,
    pub favorites: Vec<PathBuf>,
}

fn get_config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap()
        .join(".file_manager_config.json")
}

pub fn load_config() -> Result<AppConfig, Box<dyn std::error::Error>> {
    let path = get_config_path();
    if path.exists() {
        let content = fs::read_to_string(path)?;
        let config = serde_json::from_str(&content)?;
        Ok(config)
    } else {
        Ok(AppConfig::default())
    }
}

pub fn save_config(config: &AppConfig) -> Result<(), Box<dyn std::error::Error>> {
    let path = get_config_path();
    let content = serde_json::to_string_pretty(config)?;
    fs::write(path, content)?;
    Ok(())
}
