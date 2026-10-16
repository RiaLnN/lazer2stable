//! Operating system path auto-detection for osu!lazer and osu!stable.

use directories::BaseDirs;
use std::path::{Path, PathBuf};

/// Attempts to detect the default installation directory for osu!lazer.
pub fn detect_lazer_dir() -> Option<PathBuf> {
    let base_dirs = BaseDirs::new()?;

    #[cfg(target_os = "windows")]
    {
        // On Windows: %APPDATA%\osu
        let dir = base_dirs.config_dir().join("osu");
        if dir.exists() {
            return Some(dir);
        }
    }

    #[cfg(target_os = "linux")]
    {
        // 1. Native Linux: ~/.local/share/osu
        let native = base_dirs.data_dir().join("osu");
        if native.exists() {
            return Some(native);
        }

        // 2. Flatpak: ~/.var/app/sh.ppy.osu/data/osu
        let home = base_dirs.home_dir();
        let flatpak = home.join(".var/app/sh.ppy.osu/data/osu");
        if flatpak.exists() {
            return Some(flatpak);
        }
    }

    #[cfg(target_os = "macos")]
    {
        // macOS: ~/Library/Application Support/osu
        let dir = base_dirs.data_dir().join("osu");
        if dir.exists() {
            return Some(dir);
        }
    }

    None
}

/// Attempts to detect the default installation directory for osu!stable.
pub fn detect_stable_dir() -> Option<PathBuf> {
    let base_dirs = BaseDirs::new()?;

    #[cfg(target_os = "windows")]
    {
        // On Windows: %LOCALAPPDATA%\osu!
        let local_app_data = base_dirs.data_local_dir();
        let dir = local_app_data.join("osu!");
        if dir.exists() {
            return Some(dir);
        }
    }

    #[cfg(target_os = "linux")]
    {
        let home = base_dirs.home_dir();
        let candidates = [
            home.join(".wine/drive_c/osu!"),
            home.join(".wine/drive_c/Program Files/osu!"),
            home.join(".wine/drive_c/users").join(whoami()).join("AppData/Local/osu!"),
            home.join(".local/share/osu-stable"),
            home.join(".osu"),
        ];

        for candidate in candidates {
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    None
}

/// Resolves the default client.realm path from the lazer directory.
pub fn default_realm_path(lazer_dir: &Path) -> PathBuf {
    lazer_dir.join("client.realm")
}

/// Resolves the default Songs directory from the stable directory.
pub fn default_songs_dir(stable_dir: &Path) -> PathBuf {
    stable_dir.join("Songs")
}

/// Resolves the default Skins directory from the stable directory.
pub fn default_skins_dir(stable_dir: &Path) -> PathBuf {
    stable_dir.join("Skins")
}

#[cfg(target_os = "linux")]
fn whoami() -> String {
    std::env::var("USER").unwrap_or_else(|_| "user".to_string())
}
