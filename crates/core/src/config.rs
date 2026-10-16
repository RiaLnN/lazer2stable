//! Configuration management for lazer2stable.

use crate::detector;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Root configuration structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub paths: PathsConfig,

    #[serde(default)]
    pub beatmaps: BeatmapsConfig,

    #[serde(default)]
    pub skins: SkinsConfig,
}

impl Default for Config {
    fn default() -> Self {
        let lazer_dir = detector::detect_lazer_dir();
        let stable_dir = detector::detect_stable_dir();

        let realm_path = lazer_dir.as_ref().map(|p| detector::default_realm_path(p));
        let songs_dir = stable_dir.as_ref().map(|p| detector::default_songs_dir(p));
        let skins_dir = stable_dir.as_ref().map(|p| detector::default_skins_dir(p));

        Self {
            paths: PathsConfig {
                lazer_dir,
                stable_dir,
                realm_path,
                songs_dir,
                skins_dir,
                cache_dir: PathBuf::from("cache"),
                bridge_binary: None,
            },
            beatmaps: BeatmapsConfig::default(),
            skins: SkinsConfig::default(),
        }
    }
}

/// Paths configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PathsConfig {
    /// Directory of osu!lazer containing client.realm and files/.
    pub lazer_dir: Option<PathBuf>,

    /// Directory of osu!stable containing osu!.exe, Songs/, and Skins/.
    pub stable_dir: Option<PathBuf>,

    /// Path to client.realm. If None, resolves to lazer_dir/client.realm.
    pub realm_path: Option<PathBuf>,

    /// Path to Songs directory in stable. If None, resolves to stable_dir/Songs.
    pub songs_dir: Option<PathBuf>,

    /// Path to Skins directory in stable. If None, resolves to stable_dir/Skins.
    pub skins_dir: Option<PathBuf>,

    /// Directory used for caching downloaded .osz archives and temporary files.
    pub cache_dir: PathBuf,

    /// Optional explicit path to compiled realm-bridge executable.
    pub bridge_binary: Option<PathBuf>,
}

/// Beatmap migration configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BeatmapsConfig {
    /// Whether beatmap migration is enabled.
    pub enabled: bool,

    /// Beatmap source mode: "network" (online mirrors) or "local" (direct extraction from lazer store).
    pub source_mode: String,

    /// Beatmap delivery mode: "extract" (unpack to Songs/) or "open" (launch via osu!.exe).
    pub delivery_mode: String,

    /// Whether to skip downloading video files to save bandwidth and storage.
    pub no_video: bool,

    /// Whether to skip beatmaps that are already installed in stable's Songs directory.
    pub skip_existing: bool,

    /// Concurrency limit for simultaneous network downloads.
    pub concurrency: usize,

    /// Prioritized list of download mirror sources (e.g. "catboy", "nerinyan", "osudirect").
    pub sources: Vec<String>,

    /// Optional osu_session cookie for official website downloading.
    pub osu_session: Option<String>,
}

impl Default for BeatmapsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            source_mode: "network".to_string(),
            delivery_mode: "extract".to_string(),
            no_video: false,
            skip_existing: true,
            concurrency: 4,
            sources: vec![
                "catboy".to_string(),
                "nerinyan".to_string(),
                "osudirect".to_string(),
            ],
            osu_session: None,
        }
    }
}

/// Skin migration configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkinsConfig {
    /// Whether skin migration is enabled.
    pub enabled: bool,

    /// Whether to package and save exported skins as standalone .osk archives.
    pub export_as_osk: bool,

    /// Whether to directly unpack exported skins into stable's Skins directory.
    pub unpack_to_stable: bool,
}

impl Default for SkinsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            export_as_osk: true,
            unpack_to_stable: true,
        }
    }
}

impl Config {
    /// Loads configuration from the given file path, or creates default if the file does not exist.
    pub fn load_or_default(path: &Path) -> anyhow::Result<Self> {
        if !path.exists() {
            let config = Self::default();
            config.save(path)?;
            return Ok(config);
        }

        let content = fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }

    /// Saves configuration to the specified file path.
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let serialized = toml::to_string_pretty(self)?;
        fs::write(path, serialized)?;
        Ok(())
    }
}
