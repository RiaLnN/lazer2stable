# lazer2stable

> Fast, reliable, and cross-platform migration tool for transferring all your **beatmaps** and **custom skins** from **osu!lazer** into **osu!stable**.

```
client.realm ──(realm-bridge, C#)──> metadata ──(Rust core)──┬──> .osz / Songs/ (beatmaps)
                                                               └──> .osk / Skins/ (skins)
```

---

## Features

- **Desktop GUI & Command-Line CLI**: Modern, clean native graphical interface built with `egui` and vector SVG icons (no emojis), alongside a fully featured command-line interface with interactive progress bars.
- **Complete Skin Migration**: Reconstructs custom skins from lazer's content-addressed `files/` storage using dynamic Realm indexing. Exports clean `.osk` archives and automatically unpacks them into osu!stable's `Skins/` directory.
- **Intelligent Beatmap Transfer**:
  - **Network Mode (Default)**: Concurrently downloads updated `.osz` archives from high-speed mirror chains (`catboy`, `nerinyan`, `osudirect`) with automatic `429 Too Many Requests` backoff (`Retry-After`).
  - **Local Offline Mode**: Assembles `.osz` packages offline directly from lazer's local storage without requiring an internet connection.
  - **Smart Deduplication**: Automatically detects beatmaps already installed in osu!stable and skips redundant transfers.
  - **Bandwidth Saver**: Optional `--no-video` toggle to exclude large video tracks.
- **Config-Driven**: Everything is cleanly configurable via `config.toml`, with automatic path detection for Windows and Linux installations.
- **Safe & Non-Destructive**: Strictly read-only access to `client.realm`. Built-in Zip-Slip path traversal protection on all archive operations.

---

## Architecture & Project Structure

The project is structured as a modular Rust Cargo workspace backed by a companion C# Realm bridge:

```
lazer_to_stable/
├── bridge/                     # C# dynamic Realm bridge (.NET 8, Realm 20.1.0)
│   ├── BeatmapDump.csproj
│   └── Program.cs              # dump-beatmaps and dump-skins commands
├── crates/
│   ├── core/                   # lazer2stable-core (shared library)
│   │   ├── src/bridge.rs       # C# bridge execution and JSON parser
│   │   ├── src/config.rs       # TOML configuration loading and saving
│   │   ├── src/detector.rs     # OS path auto-detection (Windows/Linux)
│   │   ├── src/downloader.rs   # Async HTTP downloader with rate limiting
│   │   ├── src/beatmaps.rs     # Extraction to Songs/ and local assembly
│   │   └── src/skins.rs        # .osk packing, unpacking, name sanitization
│   ├── cli/                    # lazer2stable-cli (command-line executable)
│   └── gui/                    # lazer2stable-gui (egui desktop application)
├── config.example.toml         # Annotated configuration template
└── README.md
```

---

## Prerequisites

1. **Rust** (1.80+ or latest stable) — [Install Rust](https://rustup.rs/)
2. **.NET SDK 8.0+** — [Install .NET](https://dotnet.microsoft.com/download)

---

## Building

Build the C# realm bridge:
```bash
dotnet build -c Release
```

Build the Rust workspace (both CLI and GUI):
```bash
cargo build --release
```

The resulting binaries will be located at:
- `target/release/lazer2stable-gui` (Graphical Desktop App)
- `target/release/lazer2stable-cli` (Command-Line Tool)
- `bin/Release/net8.0/realm-bridge.dll` (Bridge)

---

## Usage

### 1. Graphical Interface (GUI)

Launch the desktop interface:
```bash
cargo run --release -p lazer2stable-gui
```

- **Beatmaps Tab**: Toggle network download vs. local offline extraction, delivery method (`extract` to Songs vs `open` in stable), video skipping, and review total detected beatmap sets.
- **Skins Tab**: View all custom skins found in lazer, toggle individual skins on/off with checkboxes, or use *Select All* / *Deselect All*.
- **Settings Tab**: Review or browse for custom directory paths (auto-detected by default) and adjust network download concurrency.
- **Bottom Bar**: Single-click **Start Migration** button with a real-time progress bar, status notifications, and an expandable collapsible log viewer.

### 2. Command-Line Interface (CLI)

Run migration with default auto-detected paths:
```bash
cargo run --release -p lazer2stable-cli
```

#### Common CLI Examples:

```bash
# Preview found skins and beatmaps without modifying disk
cargo run --release -p lazer2stable-cli -- --dry-run

# Migrate custom skins only
cargo run --release -p lazer2stable-cli -- --skins-only

# Migrate only the first 20 beatmaps without video tracks
cargo run --release -p lazer2stable-cli -- --limit 20 --no-video

# Specify custom directories manually
cargo run --release -p lazer2stable-cli -- \
  --realm "C:\Users\<user>\AppData\Roaming\osu\client.realm" \
  --stable-dir "C:\Users\<user>\AppData\Local\osu!" \
  --concurrency 6
```

#### All CLI Options:

```text
Options:
  -c, --config <CONFIG>            Path to config file [default: config.toml]
      --realm <REALM>              Path to osu!lazer client.realm database file
      --lazer-dir <LAZER_DIR>      Path to osu!lazer root directory (containing files/ and client.realm)
      --stable-dir <STABLE_DIR>    Path to osu!stable directory
      --songs-dir <SONGS_DIR>      Path to osu!stable Songs folder
      --skins-dir <SKINS_DIR>      Path to osu!stable Skins folder
      --mode <MODE>                Delivery mode for beatmaps: "extract" or "open"
      --source-mode <SOURCE_MODE>  Source mode for beatmaps: "network" or "local"
      --no-video                   Download beatmaps without video tracks
      --no-skip-installed          Do not skip beatmaps already found in stable Songs/
      --concurrency <CONCURRENCY>  Number of concurrent network downloads
      --skins-only                 Migrate only custom skins
      --beatmaps-only              Migrate only beatmap sets
      --limit <LIMIT>              Limit the number of beatmaps to process
      --dry-run                    Inspect found items without making changes
  -h, --help                       Print help
  -V, --version                    Print version
```

---

## Configuration (`config.toml`)

Copy `config.example.toml` to `config.toml` to customize default settings:

```toml
[paths]
cache_dir = "cache"

[beatmaps]
enabled = true
source_mode = "network"
delivery_mode = "extract"
no_video = false
skip_existing = true
concurrency = 4
sources = ["catboy", "nerinyan", "osudirect"]

[skins]
enabled = true
export_as_osk = true
unpack_to_stable = true
```

---

## License

MIT
