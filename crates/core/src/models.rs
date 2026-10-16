//! Data models representing dumps from osu!lazer's Realm database.

use serde::{Deserialize, Serialize};

/// Represents an individual file reference inside a skin or beatmap set.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NamedFile {
    /// Relative filename within the archive (e.g. "skin.ini", "audio.mp3").
    pub filename: String,

    /// SHA-256 hash identifying the file content in lazer's flat store.
    pub hash: String,
}

/// Represents an online beatmap set extracted from client.realm.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BeatmapSetDump {
    /// The online ID of the beatmap set.
    pub online_id: i64,

    /// Beatmap title (display metadata).
    pub title: String,

    /// Beatmap artist (display metadata).
    pub artist: String,

    /// List of named files belonging to this beatmap set.
    #[serde(default)]
    pub files: Vec<NamedFile>,
}

/// Represents a custom skin extracted from client.realm.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkinDump {
    /// Unique identifier of the skin.
    pub id: String,

    /// Name of the skin as shown in osu!.
    pub name: String,

    /// Creator or author of the skin.
    #[serde(default)]
    pub creator: String,

    /// List of named files (textures, sounds, ini) belonging to this skin.
    #[serde(default)]
    pub files: Vec<NamedFile>,
}
