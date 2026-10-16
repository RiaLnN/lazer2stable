//! Interface to invoke the C# realm-bridge tool and parse its output.

use crate::models::{BeatmapSetDump, SkinDump};
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::{debug, info};

/// Locates the realm-bridge executable or fallback runner.
pub fn find_bridge_runner(explicit_path: Option<&Path>) -> Result<(PathBuf, Vec<String>)> {
    if let Some(path) = explicit_path {
        if path.exists() {
            return Ok((path.to_path_buf(), vec![]));
        }
    }

    // Check beside current executable
    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(parent) = current_exe.parent() {
            let beside = parent.join("realm-bridge");
            if beside.exists() {
                return Ok((beside, vec![]));
            }
            let beside_exe = parent.join("realm-bridge.exe");
            if beside_exe.exists() {
                return Ok((beside_exe, vec![]));
            }
        }
    }

    // Search starting from current directory up to 3 parent directories
    let mut search_roots = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        let mut curr = Some(cwd.as_path());
        for _ in 0..4 {
            if let Some(c) = curr {
                search_roots.push(c.to_path_buf());
                curr = c.parent();
            }
        }
    }

    let rel_candidates = [
        "bin/Release/net8.0/realm-bridge",
        "bin/Release/net8.0/realm-bridge.exe",
        "bridge/bin/Release/net8.0/realm-bridge",
        "bridge/bin/Release/net8.0/realm-bridge.exe",
    ];

    for root in &search_roots {
        for candidate in rel_candidates {
            let path = root.join(candidate);
            if path.exists() {
                return Ok((path, vec![]));
            }
        }
    }

    let dll_candidates = [
        "bin/Release/net8.0/realm-bridge.dll",
        "bridge/bin/Release/net8.0/realm-bridge.dll",
    ];

    for root in &search_roots {
        for candidate in dll_candidates {
            let path = root.join(candidate);
            if path.exists() {
                return Ok((PathBuf::from("dotnet"), vec![path.to_string_lossy().to_string()]));
            }
        }
    }

    let csproj_candidates = [
        "BeatmapDump.csproj",
        "bridge/BeatmapDump.csproj",
    ];

    for root in &search_roots {
        for candidate in csproj_candidates {
            let path = root.join(candidate);
            if path.exists() {
                return Ok((
                    PathBuf::from("dotnet"),
                    vec![
                        "run".to_string(),
                        "--project".to_string(),
                        path.to_string_lossy().to_string(),
                        "-c".to_string(),
                        "Release".to_string(),
                        "--".to_string(),
                    ],
                ));
            }
        }
    }

    bail!("Could not find realm-bridge binary or BeatmapDump.csproj. Please build the bridge with `dotnet build -c Release`.")
}

/// Runs realm-bridge with the given command and returns raw stdout string.
pub fn run_bridge(
    command: &str,
    realm_path: &Path,
    bridge_binary: Option<&Path>,
) -> Result<String> {
    if !realm_path.exists() {
        bail!("Realm database does not exist at path: {}", realm_path.display());
    }

    let (program, base_args) = find_bridge_runner(bridge_binary)?;
    debug!("Using realm-bridge runner: {} with base args {:?}", program.display(), base_args);

    let mut cmd = Command::new(&program);
    for arg in &base_args {
        cmd.arg(arg);
    }
    cmd.arg(command);
    cmd.arg("--realm");
    cmd.arg(realm_path);

    info!("Executing realm-bridge command '{}'", command);
    let output = cmd.output().with_context(|| {
        format!("Failed to execute realm-bridge using '{}'", program.display())
    })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("realm-bridge exited with status {}: {}", output.status, stderr);
    }

    let stdout = String::from_utf8(output.stdout)
        .context("Failed to parse realm-bridge stdout as UTF-8")?;

    Ok(stdout)
}

/// Fetches all online beatmap sets from client.realm.
pub fn fetch_beatmaps(
    realm_path: &Path,
    bridge_binary: Option<&Path>,
) -> Result<Vec<BeatmapSetDump>> {
    let json_str = run_bridge("dump-beatmaps", realm_path, bridge_binary)?;
    let beatmaps: Vec<BeatmapSetDump> = serde_json::from_str(&json_str)
        .context("Failed to deserialize beatmap sets JSON from realm-bridge")?;
    Ok(beatmaps)
}

/// Fetches all custom skins from client.realm.
pub fn fetch_skins(
    realm_path: &Path,
    bridge_binary: Option<&Path>,
) -> Result<Vec<SkinDump>> {
    let json_str = run_bridge("dump-skins", realm_path, bridge_binary)?;
    let skins: Vec<SkinDump> = serde_json::from_str(&json_str)
        .context("Failed to deserialize skins JSON from realm-bridge")?;
    Ok(skins)
}
