//! Skin packaging (.osk) and extraction utilities.

use crate::models::SkinDump;
use anyhow::{bail, Context, Result};
use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use tracing::{debug, warn};
use zip::write::SimpleFileOptions;
use zip::CompressionMethod;

/// Sanitizes a skin or beatmap name to be a safe directory/file name across Windows and Linux.
pub fn sanitize_name(name: &str) -> String {
    let forbidden = ['/', '\\', '<', '>', ':', '"', '|', '?', '*'];
    let sanitized: String = name
        .chars()
        .map(|c| if forbidden.contains(&c) { '_' } else { c })
        .collect();

    // Windows disallows trailing dots and spaces in file and folder names
    let trimmed = sanitized.trim().trim_end_matches('.');
    if trimmed.is_empty() {
        "unnamed_skin".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Locates a file in osu!lazer's content-addressed store given its SHA-256 hash.
pub fn locate_lazer_file(files_dir: &Path, hash: &str) -> Option<PathBuf> {
    if hash.is_empty() {
        return None;
    }

    // Modern lazer path: files/<first_char>/<first_two_chars>/<hash>
    if hash.len() >= 2 {
        let p1 = files_dir.join(&hash[0..1]).join(&hash[0..2]).join(hash);
        if p1.is_file() {
            return Some(p1);
        }

        // Alternative lazer path: files/<first_char>/<hash>
        let p2 = files_dir.join(&hash[0..1]).join(hash);
        if p2.is_file() {
            return Some(p2);
        }
    }

    // Flat path: files/<hash>
    let p3 = files_dir.join(hash);
    if p3.is_file() {
        return Some(p3);
    }

    None
}

/// Exports a single skin: creates an .osk archive and/or unpacks it into stable's Skins directory.
pub fn export_skin(
    skin: &SkinDump,
    lazer_files_dir: &Path,
    out_osk_path: Option<&Path>,
    target_skins_dir: Option<&Path>,
) -> Result<()> {
    let safe_name = sanitize_name(&skin.name);

    // 1. Pack into .osk (ZIP archive) if requested
    if let Some(osk_path) = out_osk_path {
        if let Some(parent) = osk_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let osk_file = File::create(osk_path)
            .with_context(|| format!("Failed to create .osk file at {}", osk_path.display()))?;
        let mut zip = zip::ZipWriter::new(BufWriter::new(osk_file));
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .unix_permissions(0o644);

        let mut buffer = Vec::new();
        for named_file in &skin.files {
            if let Some(source_path) = locate_lazer_file(lazer_files_dir, &named_file.hash) {
                // Normalize relative filename inside zip
                let clean_filename = named_file.filename.replace('\\', "/");
                zip.start_file(&clean_filename, options)?;
                buffer.clear();
                let mut f = File::open(&source_path)?;
                f.read_to_end(&mut buffer)?;
                zip.write_all(&buffer)?;
            } else {
                warn!(
                    "Skin '{}': file '{}' with hash '{}' not found in lazer storage",
                    skin.name, named_file.filename, named_file.hash
                );
            }
        }
        zip.finish()?;
        debug!("Successfully packaged .osk for skin '{}'", skin.name);
    }

    // 2. Unpack directly to stable's Skins directory if requested
    if let Some(skins_dir) = target_skins_dir {
        let dest_folder = skins_dir.join(&safe_name);
        fs::create_dir_all(&dest_folder)?;
        let canonical_dest = dest_folder.canonicalize()?;

        for named_file in &skin.files {
            let clean_filename = named_file.filename.replace('\\', "/");
            let target_file_path = dest_folder.join(&clean_filename);

            // Zip-Slip protection: ensure target is strictly inside canonical_dest
            if let Some(parent) = target_file_path.parent() {
                fs::create_dir_all(parent)?;
                let canonical_parent = parent.canonicalize()?;
                if !canonical_parent.starts_with(&canonical_dest) {
                    bail!(
                        "Zip-Slip attempt detected: file '{}' targets outside skin directory",
                        clean_filename
                    );
                }
            }

            if let Some(source_path) = locate_lazer_file(lazer_files_dir, &named_file.hash) {
                fs::copy(&source_path, &target_file_path)?;
            } else {
                warn!(
                    "Skin '{}': file '{}' missing for unpack",
                    skin.name, named_file.filename
                );
            }
        }
        debug!(
            "Successfully unpacked skin '{}' to {}",
            skin.name,
            dest_folder.display()
        );
    }

    Ok(())
}
