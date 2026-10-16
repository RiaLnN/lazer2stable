# lazer2stable Rust Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a robust, cross-platform Rust application with an `egui` GUI and CLI to migrate beatmaps and skins from osu! lazer into osu! stable using a standalone C# Realm bridge.

**Architecture:** A Cargo workspace containing a shared core library (`lazer2stable-core`), a CLI executable (`lazer2stable-cli`), and an `egui` desktop GUI (`lazer2stable-gui`). Reading osu! lazer's MongoDB Realm database is handled by a companion C# tool (`realm-bridge`), while all network downloads, file management, zip/osk creation, and UI interactions are performed asynchronously in Rust.

**Tech Stack:** Rust 2021, `egui` / `eframe`, `egui_extras` (SVG rendering), `tokio`, `reqwest`, `zip`, `serde` / `toml`, `clap`, C# / .NET 8 (`Realms` 20.1.0).

**Spec:** `docs/superpowers/specs/2026-10-16-lazer-to-stable-design.md`

## Global Constraints

- All user-facing UI labels, tooltips, dialogs, status messages, CLI output, and codebase comments/docstrings must be written in English.
- Strictly read-only access to `client.realm` (no write or schema modifications).
- Non-blocking UI: all file I/O, database queries, and network downloads run on background async worker threads with message channels to the UI.
- Secure archive extraction: all zip extraction logic must validate canonical destination paths to guard against Zip-Slip vulnerabilities.

## Review Focus

- Corrupted or locked `client.realm` (e.g. osu! lazer running): the bridge must report clear errors without crashing Rust.
- Missing files in lazer's `files/` store: skin and map assembly must report missing file hashes gracefully rather than panicking.
- HTTP 429 Rate Limiting: the downloader must parse `Retry-After` headers and sleep appropriately.
- Invalid paths: non-existent or inaccessible stable/lazer directories must be flagged with visual error indicators.
- Special characters in beatmap/skin titles: archive filenames and directory paths must be properly sanitized across Windows and Linux.

---

### Task 1: C# Realm Bridge Expansion (`dump-skins` and file lists)

**Files:**
- Modify: `Program.cs`
- Modify: `BeatmapDump.csproj`
- Test: manual invocation against `client.realm`

**Interfaces:**
- Produces: CLI commands:
  - `realm-bridge dump-beatmaps --realm <path>` -> JSON `[{"online_id": int, "title": string, "artist": string, "files": [{"filename": string, "hash": string}]}]`
  - `realm-bridge dump-skins --realm <path>` -> JSON `[{"id": string, "name": string, "creator": string, "files": [{"filename": string, "hash": string}]}]`

- [ ] **Step 1: Update `Program.cs` to implement `dump-skins` and include file lists in `dump-beatmaps`**

Update `Program.cs` to add:
1. Command switch `case "dump-skins": return DumpSkins(realmPath);`
2. `DumpSkins` function reading `realm.DynamicApi.All("SkinInfo")`, checking `DeletePending`, extracting `ID`, `Name`, `Creator`, and `Files` (`RealmNamedFileUsage` -> `Filename`, `File.Hash`).
3. Enhance `DumpBeatmaps` to also include `Files` for each `BeatmapSet` for optional local extraction.

- [ ] **Step 2: Build the C# bridge project**

Run: `dotnet build -c Release`
Expected: Build succeeded with 0 errors.

- [ ] **Step 3: Test `dump-skins` against real `client.realm`**

Run: `dotnet run -c Release -- dump-skins --realm /home/riann/.var/app/sh.ppy.osu/data/osu/client.realm | head -c 300`
Expected: JSON output array containing skin objects with name and files.

- [ ] **Step 4: Test `dump-beatmaps` output**

Run: `dotnet run -c Release -- dump-beatmaps --realm /home/riann/.var/app/sh.ppy.osu/data/osu/client.realm | head -c 300`
Expected: JSON output array containing online_id and files.

- [ ] **Step 5: Commit bridge changes**

```bash
git add Program.cs BeatmapDump.csproj
git commit -m "feat(bridge): add dump-skins command and beatmap file mapping"
```

---

### Task 2: Cargo Workspace & Core Config / Path Detector

**Files:**
- Create: `Cargo.toml` (workspace manifest)
- Create: `crates/core/Cargo.toml`
- Create: `crates/core/src/lib.rs`
- Create: `crates/core/src/config.rs`
- Create: `crates/core/src/detector.rs`
- Test: `crates/core/tests/config_test.rs`

**Interfaces:**
- Consumes: Standard filesystem APIs
- Produces:
  - `struct Config { paths: PathsConfig, beatmaps: BeatmapsConfig, skins: SkinsConfig }`
  - `Config::load_or_default(path: &Path) -> Result<Config>`
  - `Config::save(&self, path: &Path) -> Result<()>`
  - `detector::detect_lazer_dir() -> Option<PathBuf>`
  - `detector::detect_stable_dir() -> Option<PathBuf>`

- [ ] **Step 1: Write failing unit test for configuration loading and default detection**

Create `crates/core/tests/config_test.rs`:
```rust
use lazer2stable_core::config::Config;
use std::path::PathBuf;

#[test]
fn test_default_config_serialization() {
    let config = Config::default();
    let toml_str = toml::to_string(&config).expect("failed to serialize");
    let deserialized: Config = toml::from_str(&toml_str).expect("failed to deserialize");
    assert_eq!(config.beatmaps.concurrency, deserialized.beatmaps.concurrency);
    assert_eq!(config.beatmaps.source_mode, "network");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p lazer2stable-core --test config_test`
Expected: FAIL (crate or module not found).

- [ ] **Step 3: Implement workspace manifest, `config.rs`, and `detector.rs` in English**

1. Create root `Cargo.toml` with `[workspace] members = ["crates/core", "crates/cli", "crates/gui"]`.
2. Create `crates/core/Cargo.toml` with dependencies: `serde`, `toml`, `directories`, `thiserror`, `anyhow`, `tracing`.
3. Implement `crates/core/src/config.rs` with `PathsConfig`, `BeatmapsConfig`, `SkinsConfig`.
4. Implement `crates/core/src/detector.rs` with OS-specific path detectors (Windows `%APPDATA%\osu`, Linux `~/.local/share/osu` and `~/.var/app/sh.ppy.osu/data/osu`, Wine prefixes for stable).

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p lazer2stable-core --test config_test`
Expected: PASS.

- [ ] **Step 5: Commit core config and detector**

```bash
git add Cargo.toml crates/core/
git commit -m "feat(core): setup workspace, config model, and directory detectors"
```

---

### Task 3: C# Bridge Invoker & JSON Parser in Core

**Files:**
- Create: `crates/core/src/bridge.rs`
- Create: `crates/core/src/models.rs`
- Test: `crates/core/tests/bridge_test.rs`

**Interfaces:**
- Consumes: `bridge::run_bridge(bridge_path: Option<&Path>, command: &str, realm_path: &Path) -> Result<String>`
- Produces:
  - `models::BeatmapSetDump { online_id: i64, title: String, artist: String, files: Vec<NamedFile> }`
  - `models::SkinDump { id: String, name: String, creator: String, files: Vec<NamedFile> }`
  - `bridge::fetch_beatmaps(realm_path: &Path, bridge_binary: Option<&Path>) -> Result<Vec<BeatmapSetDump>>`
  - `bridge::fetch_skins(realm_path: &Path, bridge_binary: Option<&Path>) -> Result<Vec<SkinDump>>`

- [ ] **Step 1: Write test for models deserialization**

Create `crates/core/tests/bridge_test.rs` asserting deserialization of sample JSON output from `dump-skins` and `dump-beatmaps`.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p lazer2stable-core --test bridge_test`
Expected: FAIL.

- [ ] **Step 3: Implement `models.rs` and `bridge.rs` in English**

1. Implement `NamedFile { filename: String, hash: String }`.
2. Implement bridge locator cascade (configured path -> beside exe -> `bridge/bin/Release/net8.0/` -> `dotnet run`).
3. Invoke process using `std::process::Command`, capturing stdout and checking status.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p lazer2stable-core --test bridge_test`
Expected: PASS.

- [ ] **Step 5: Commit bridge integration**

```bash
git add crates/core/src/bridge.rs crates/core/src/models.rs crates/core/tests/bridge_test.rs
git commit -m "feat(core): add realm bridge process invoker and parser"
```

---

### Task 4: Skin Exporter (.osk Pack & Unpack)

**Files:**
- Create: `crates/core/src/skins.rs`
- Test: `crates/core/tests/skins_test.rs`

**Interfaces:**
- Consumes: `SkinDump`, `lazer_dir: &Path`, `stable_dir: &Path`
- Produces:
  - `skins::export_skin(skin: &SkinDump, lazer_files_dir: &Path, out_osk_path: Option<&Path>, target_skins_dir: Option<&Path>) -> Result<()>`
  - `skins::locate_lazer_file(files_dir: &Path, hash: &str) -> Option<PathBuf>`

- [ ] **Step 1: Write unit test for skin packaging and Zip-Slip protection**

Create `crates/core/tests/skins_test.rs`:
- Mock temporary directory with a dummy hashed file (`files/ab/ab12...`).
- Verify that `export_skin` packages the file into `.osk` with correct relative name.
- Verify unpacking creates the expected files in `{target_skins_dir}/{skin_name}`.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p lazer2stable-core --test skins_test`
Expected: FAIL.

- [ ] **Step 3: Implement `skins.rs` using `zip` crate in English**

1. Implement `locate_lazer_file`: check `{files}/{hash[0..1]}/{hash[0..2]}/{hash}` and `{files}/{hash[0..1]}/{hash}`.
2. Build zip archive `.osk` writing each file with `CompressionMethod::Deflated`.
3. If `target_skins_dir` is provided, unpack directly into destination folder ensuring canonical path check.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p lazer2stable-core --test skins_test`
Expected: PASS.

- [ ] **Step 5: Commit skin exporter**

```bash
git add crates/core/src/skins.rs crates/core/tests/skins_test.rs
git commit -m "feat(core): implement skin osk packaging and direct export"
```

---

### Task 5: Beatmaps Downloader & Mirror Pipeline

**Files:**
- Create: `crates/core/src/downloader.rs`
- Create: `crates/core/src/beatmaps.rs`
- Test: `crates/core/tests/beatmaps_test.rs`

**Interfaces:**
- Consumes: `reqwest`, `tokio`, `BeatmapSetDump`
- Produces:
  - `downloader::DownloadSource` enum (`Catboy`, `Nerinyan`, `OsuDirect`, `Custom(String)`)
  - `downloader::download_beatmap(client: &Client, id: i64, sources: &[DownloadSource], no_video: bool, cache_file: &Path) -> Result<()>`
  - `beatmaps::extract_to_songs(osz_path: &Path, songs_dir: &Path, id: i64) -> Result<()>`
  - `beatmaps::assemble_from_lazer(set: &BeatmapSetDump, lazer_files_dir: &Path, target_osz: &Path) -> Result<()>`

- [ ] **Step 1: Write unit test for beatmap extraction to Songs and local assembly**

Create `crates/core/tests/beatmaps_test.rs`:
- Test `extract_to_songs` with a generated zip file.
- Test `installed_in_stable` scanner that scans `{songs_dir}` for folders starting with `<id> `.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p lazer2stable-core --test beatmaps_test`
Expected: FAIL.

- [ ] **Step 3: Implement `downloader.rs` and `beatmaps.rs` in English**

1. Implement mirror URL builders:
   - `catboy`: `https://catboy.best/d/{id}`
   - `nerinyan`: `https://api.nerinyan.moe/d/{id}`
   - `osudirect`: `https://api.osu.direct/d/{id}`
2. Handle status 429: inspect `Retry-After` header, apply backoff.
3. Implement `extract_to_songs` safely with Zip-Slip protection.
4. Implement `assemble_from_lazer` to build `.osz` locally from `files/`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p lazer2stable-core --test beatmaps_test`
Expected: PASS.

- [ ] **Step 5: Commit beatmap downloader and extractor**

```bash
git add crates/core/src/downloader.rs crates/core/src/beatmaps.rs crates/core/tests/beatmaps_test.rs
git commit -m "feat(core): implement beatmap download pipeline and extraction"
```

---

### Task 6: CLI Binary (`lazer2stable-cli`)

**Files:**
- Create: `crates/cli/Cargo.toml`
- Create: `crates/cli/src/main.rs`
- Test: manual integration test with `--help` and dry-run

**Interfaces:**
- Consumes: `lazer2stable_core`
- Produces: Executable `lazer2stable-cli` with CLI arguments:
  `--realm <PATH>`, `--stable-dir <PATH>`, `--songs-dir <PATH>`, `--mode <extract|open>`, `--no-video`, `--concurrency <N>`, `--skins`, `--beatmaps`

- [ ] **Step 1: Create `crates/cli/Cargo.toml` and CLI parser**

Setup dependencies: `lazer2stable-core`, `clap` (derive), `tokio` (full), `indicatif`, `tracing-subscriber`.

- [ ] **Step 2: Implement `crates/cli/src/main.rs` in English**

1. Parse CLI arguments.
2. Load configuration, overriding with explicit CLI flags.
3. Execute migration: dump beatmaps/skins, display progress bars via `indicatif`.
4. Handle SIGINT gracefully.

- [ ] **Step 3: Test CLI help and dry-run**

Run: `cargo run -p lazer2stable-cli -- --help`
Expected: Displays English usage instructions with options.

- [ ] **Step 4: Commit CLI**

```bash
git add crates/cli/
git commit -m "feat(cli): implement command-line interface with progress tracking"
```

---

### Task 7: GUI Application with egui & SVG Icons (`lazer2stable-gui`)

**Files:**
- Create: `crates/gui/Cargo.toml`
- Create: `crates/gui/src/main.rs`
- Create: `crates/gui/src/app.rs`
- Create: `crates/gui/src/icons.rs`
- Create: `crates/gui/src/tabs/beatmaps.rs`
- Create: `crates/gui/src/tabs/skins.rs`
- Create: `crates/gui/src/tabs/settings.rs`
- Create: `crates/gui/assets/*.svg`

**Interfaces:**
- Consumes: `lazer2stable_core`, `eframe`, `egui`, `egui_extras`
- Produces: Executable `lazer2stable-gui`

- [ ] **Step 1: Setup `crates/gui/Cargo.toml` and asset icons**

1. Add dependencies: `lazer2stable-core`, `eframe`, `egui`, `egui_extras` (with `svg`), `image`, `tokio`, `crossbeam-channel`.
2. Add clean vector SVG icons in `crates/gui/assets/`:
   - `beatmaps.svg` (music / disc icon)
   - `skins.svg` (palette icon)
   - `settings.svg` (cog icon)
   - `folder.svg`, `check.svg`, `alert.svg`

- [ ] **Step 2: Implement UI tabs and main application loop in English**

1. Dark theme styling with osu! pink accent (`#FF66AA`).
2. Tab 1 (Beatmaps): Toggles for network vs local, no-video, skip existing, list summary.
3. Tab 2 (Skins): Detected skins with checkboxes, select all / deselect all.
4. Tab 3 (Settings): Directory pickers (using `rfd` dialogs), concurrency slider, mirror selector, Save Config button.
5. Bottom action bar: Progress bar, status text, Start Migration / Cancel button, expandable log viewer.
6. Async task channel: background tokio runtime sends `ProgressUpdate` events to egui.

- [ ] **Step 3: Compile and run GUI check**

Run: `cargo check -p lazer2stable-gui`
Expected: Compiles with 0 errors.

- [ ] **Step 4: Commit GUI**

```bash
git add crates/gui/
git commit -m "feat(gui): implement egui desktop interface with tabs and svg icons"
```

---

### Task 8: End-to-End Integration, Validation & Documentation

**Files:**
- Modify: `README.md` (rewrite in English)
- Create: `config.example.toml`
- Test: Full cargo build and test run

- [ ] **Step 1: Run all tests in the workspace**

Run: `cargo test --workspace`
Expected: All tests pass.

- [ ] **Step 2: Verify `lazer2stable-cli` can scan local lazer Realm**

Run: `cargo run -p lazer2stable-cli -- --realm /home/riann/.var/app/sh.ppy.osu/data/osu/client.realm --limit 5 --dry-run` (or inspect scan).
Expected: Reports detected beatmaps and skins in English.

- [ ] **Step 3: Update `README.md` and `config.example.toml` in English**

Update `README.md` explaining CLI & GUI usage, compilation steps, configuration options, and architecture overview.

- [ ] **Step 4: Final commit**

```bash
git add README.md config.example.toml
git commit -m "docs: rewrite README in English and add example configuration"
```
