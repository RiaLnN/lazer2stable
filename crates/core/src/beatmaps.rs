//! Beatmap extraction, local assembly, and installation utilities.

use crate::models::BeatmapSetDump;
use crate::skins::locate_lazer_file;
use anyhow::{bail, Context, Result};
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use tracing::{debug, warn};
use zip::write::SimpleFileOptions;
use zip::CompressionMethod;
use zip::ZipArchive;

/// Scans the given Songs directory in osu!stable and returns a set of all installed beatmap set IDs.
pub fn installed_in_stable(songs_dir: &Path) -> HashSet<i64> {
    let mut installed = HashSet::new();

    let Ok(entries) = fs::read_dir(songs_dir) else {
        return installed;
    };

    for entry in entries.flatten() {
        if let Ok(file_type) = entry.file_type() {
            if file_type.is_dir() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                // Match prefix digits (e.g. "12345 Title", "12345", "12345_Title")
                let id_str = name_str
                    .split(|c: char| !c.is_ascii_digit())
                    .next()
                    .unwrap_or("");

                if let Ok(id) = id_str.parse::<i64>() {
                    if id > 0 {
                        installed.insert(id);
                    }
                }
            }
        }
    }

    installed
}

/// Extracts a .osz (ZIP) archive into stable's Songs directory with Zip-Slip protection.
pub fn extract_to_songs(osz_path: &Path, songs_dir: &Path, id: i64) -> Result<PathBuf> {
    let target_dir = songs_dir.join(id.to_string());
    fs::create_dir_all(&target_dir)?;
    let canonical_target = target_dir.canonicalize()?;

    let file = File::open(osz_path)
        .with_context(|| format!("Failed to open .osz archive at {}", osz_path.display()))?;
    let mut archive = ZipArchive::new(file)?;

    for i in 0..archive.len() {
        let mut member = archive.by_index(i)?;
        let Some(enclosed_name) = member.enclosed_name() else {
            continue;
        };

        let out_path = target_dir.join(enclosed_name);

        if member.is_dir() {
            fs::create_dir_all(&out_path)?;
            continue;
        }

        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
            let canonical_parent = parent.canonicalize()?;
            if !canonical_parent.starts_with(&canonical_target) {
                bail!("Zip-Slip attempt detected: '{:?}'", member.name());
            }
        }

        let mut outfile = File::create(&out_path)?;
        std::io::copy(&mut member, &mut outfile)?;
    }

    Ok(target_dir)
}

/// Assembles a .osz (ZIP archive) directly from files in osu!lazer's content-addressed storage.
pub fn assemble_from_lazer(
    set: &BeatmapSetDump,
    lazer_files_dir: &Path,
    target_osz: &Path,
) -> Result<PathBuf> {
    if let Some(parent) = target_osz.parent() {
        fs::create_dir_all(parent)?;
    }

    let file = File::create(target_osz)?;
    let mut zip = zip::ZipWriter::new(BufWriter::new(file));
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o644);

    let mut buffer = Vec::new();
    for named_file in &set.files {
        if let Some(source_path) = locate_lazer_file(lazer_files_dir, &named_file.hash) {
            let clean_filename = named_file.filename.replace('\\', "/");
            zip.start_file(&clean_filename, options)?;
            buffer.clear();
            let mut f = File::open(&source_path)?;
            f.read_to_end(&mut buffer)?;
            zip.write_all(&buffer)?;
        } else {
            warn!(
                "Beatmap set {}: file '{}' with hash '{}' missing in lazer storage",
                set.online_id, named_file.filename, named_file.hash
            );
        }
    }

    zip.finish()?;
    debug!("Successfully assembled local .osz for set {}", set.online_id);
    Ok(target_osz.to_path_buf())
}

/// Opens a .osz file in osu!stable via system file association or direct process invocation.
pub fn open_in_stable(osz_path: &Path, stable_exe: Option<&Path>) -> Result<()> {
    if let Some(exe) = stable_exe {
        if exe.is_file() {
            std::process::Command::new(exe).arg(osz_path).spawn()?;
            return Ok(());
        }
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer").arg(osz_path).spawn()?;
        return Ok(());
    }

    #[cfg(target_os = "linux")]
    {
        // Try xdg-open on Linux
        std::process::Command::new("xdg-open").arg(osz_path).spawn()?;
        return Ok(());
    }

    #[allow(unreachable_code)]
    Ok(())
}
