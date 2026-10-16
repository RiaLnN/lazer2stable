//! Command-line interface for migrating beatmaps and skins from osu!lazer to osu!stable.

use anyhow::{bail, Context, Result};
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use lazer2stable_core::beatmaps::{extract_to_songs, installed_in_stable, open_in_stable};
use lazer2stable_core::bridge::{fetch_beatmaps, fetch_skins};
use lazer2stable_core::config::Config;
use lazer2stable_core::detector;
use lazer2stable_core::downloader::{create_http_client, download_beatmap};
use lazer2stable_core::skins::export_skin;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::Semaphore;

#[derive(Parser, Debug)]
#[command(name = "lazer2stable-cli")]
#[command(author = "lazer2stable contributors")]
#[command(version = "2.0.0")]
#[command(about = "Migrate beatmaps and skins from osu!lazer to osu!stable", long_about = None)]
struct CliArgs {
    /// Path to config file
    #[arg(short, long, default_value = "config.toml")]
    config: PathBuf,

    /// Path to osu!lazer client.realm database file
    #[arg(long)]
    realm: Option<PathBuf>,

    /// Path to osu!lazer root directory (containing files/ and client.realm)
    #[arg(long)]
    lazer_dir: Option<PathBuf>,

    /// Path to osu!stable directory
    #[arg(long)]
    stable_dir: Option<PathBuf>,

    /// Path to osu!stable Songs folder
    #[arg(long)]
    songs_dir: Option<PathBuf>,

    /// Path to osu!stable Skins folder
    #[arg(long)]
    skins_dir: Option<PathBuf>,

    /// Delivery mode for beatmaps: "extract" (unpack to Songs/) or "open" (launch via osu!.exe)
    #[arg(long)]
    mode: Option<String>,

    /// Source mode for beatmaps: "network" (download from mirrors) or "local" (assemble from lazer store)
    #[arg(long)]
    source_mode: Option<String>,

    /// Download beatmaps without video tracks to save bandwidth and disk space
    #[arg(long)]
    no_video: bool,

    /// Do not skip beatmaps already found in stable's Songs directory
    #[arg(long)]
    no_skip_installed: bool,

    /// Number of concurrent network downloads
    #[arg(long)]
    concurrency: Option<usize>,

    /// Migrate only custom skins
    #[arg(long)]
    skins_only: bool,

    /// Migrate only beatmap sets
    #[arg(long)]
    beatmaps_only: bool,

    /// Limit the number of beatmaps to process (useful for testing)
    #[arg(long)]
    limit: Option<usize>,

    /// Inspect and print found items without making changes to disk
    #[arg(long)]
    dry_run: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let args = CliArgs::parse();

    let mut config = Config::load_or_default(&args.config)
        .context("Failed to load or initialize config file")?;

    // Override config with explicit CLI flags
    if let Some(r) = args.realm {
        config.paths.realm_path = Some(r);
    }
    if let Some(l) = args.lazer_dir {
        config.paths.lazer_dir = Some(l);
    }
    if let Some(s) = args.stable_dir {
        config.paths.stable_dir = Some(s);
    }
    if let Some(s) = args.songs_dir {
        config.paths.songs_dir = Some(s);
    }
    if let Some(s) = args.skins_dir {
        config.paths.skins_dir = Some(s);
    }
    if let Some(m) = args.mode {
        config.beatmaps.delivery_mode = m;
    }
    if let Some(sm) = args.source_mode {
        config.beatmaps.source_mode = sm;
    }
    if args.no_video {
        config.beatmaps.no_video = true;
    }
    if args.no_skip_installed {
        config.beatmaps.skip_existing = false;
    }
    if let Some(c) = args.concurrency {
        config.beatmaps.concurrency = c;
    }

    let migrate_skins = !args.beatmaps_only && config.skins.enabled;
    let migrate_beatmaps = !args.skins_only && config.beatmaps.enabled;

    println!("==================================================");
    println!("       lazer2stable - Migration Tool v2.0         ");
    println!("==================================================");

    // Resolve paths
    let lazer_dir = config
        .paths
        .lazer_dir
        .clone()
        .or_else(detector::detect_lazer_dir);
    let stable_dir = config
        .paths
        .stable_dir
        .clone()
        .or_else(detector::detect_stable_dir);

    let Some(lazer_dir) = lazer_dir else {
        bail!("Could not detect osu!lazer directory. Please specify with --lazer-dir");
    };

    let realm_path = config
        .paths
        .realm_path
        .clone()
        .unwrap_or_else(|| detector::default_realm_path(&lazer_dir));

    if !realm_path.is_file() {
        bail!("client.realm not found at {}", realm_path.display());
    }

    let files_dir = lazer_dir.join("files");
    let stable_dir_ref = stable_dir.as_deref();

    let songs_dir = config
        .paths
        .songs_dir
        .clone()
        .or_else(|| stable_dir_ref.map(detector::default_songs_dir));

    let skins_dir = config
        .paths
        .skins_dir
        .clone()
        .or_else(|| stable_dir_ref.map(detector::default_skins_dir));

    println!("Lazer directory : {}", lazer_dir.display());
    println!("Realm database  : {}", realm_path.display());
    if let Some(ref s) = stable_dir {
        println!("Stable directory: {}", s.display());
    }
    if let Some(ref s) = songs_dir {
        println!("Songs directory : {}", s.display());
    }
    if let Some(ref s) = skins_dir {
        println!("Skins directory : {}", s.display());
    }
    println!("--------------------------------------------------");

    // 1. Process Skins
    if migrate_skins {
        println!("Scanning custom skins from client.realm...");
        let skins = fetch_skins(&realm_path, config.paths.bridge_binary.as_deref())
            .context("Failed to fetch skins from realm")?;

        println!("Found {} custom skin(s).", skins.len());

        if args.dry_run {
            for skin in &skins {
                println!(" - [Skin] '{}' ({} files)", skin.name, skin.files.len());
            }
        } else if !skins.is_empty() {
            let pb = ProgressBar::new(skins.len() as u64);
            pb.set_style(
                ProgressStyle::default_bar()
                    .template("[{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta}) {msg}")?
                    .progress_chars("#>-"),
            );

            let osk_export_dir = config.paths.cache_dir.join("skins");
            for skin in &skins {
                pb.set_message(skin.name.clone());
                let osk_path = if config.skins.export_as_osk {
                    let safe = lazer2stable_core::skins::sanitize_name(&skin.name);
                    Some(osk_export_dir.join(format!("{}.osk", safe)))
                } else {
                    None
                };

                let target_skins = if config.skins.unpack_to_stable {
                    skins_dir.as_deref()
                } else {
                    None
                };

                if let Err(err) = export_skin(skin, &files_dir, osk_path.as_deref(), target_skins) {
                    eprintln!("\nError exporting skin '{}': {}", skin.name, err);
                }
                pb.inc(1);
            }
            pb.finish_with_message("Skin migration finished.");
            println!("Custom skins migrated successfully.");
        }
        println!("--------------------------------------------------");
    }

    // 2. Process Beatmaps
    if migrate_beatmaps {
        println!("Scanning online beatmaps from client.realm...");
        let mut beatmaps = fetch_beatmaps(&realm_path, config.paths.bridge_binary.as_deref())
            .context("Failed to fetch beatmaps from realm")?;

        let total_in_realm = beatmaps.len();
        println!("Found {} online beatmap set(s) in lazer.", total_in_realm);

        if config.beatmaps.skip_existing {
            if let Some(ref s_dir) = songs_dir {
                let existing = installed_in_stable(s_dir);
                let before = beatmaps.len();
                beatmaps.retain(|b| !existing.contains(&b.online_id));
                println!(
                    "Skipping {} beatmap set(s) already installed in stable.",
                    before - beatmaps.len()
                );
            }
        }

        if let Some(limit) = args.limit {
            beatmaps.truncate(limit);
            println!("Applied limit: processing first {} beatmap(s).", limit);
        }

        println!("To process: {} beatmap set(s).", beatmaps.len());

        if args.dry_run {
            for set in &beatmaps {
                println!(
                    " - [Beatmap] {} - {} ({})",
                    set.artist, set.title, set.online_id
                );
            }
        } else if !beatmaps.is_empty() {
            let Some(ref songs_path) = songs_dir else {
                bail!("Songs directory must be specified for beatmap migration");
            };

            let pb = ProgressBar::new(beatmaps.len() as u64);
            pb.set_style(
                ProgressStyle::default_bar()
                    .template(
                        "[{elapsed_precise}] [{bar:40.magenta/purple}] {pos}/{len} ({eta}) {msg}",
                    )?
                    .progress_chars("#>-"),
            );

            let http_client = create_http_client(config.beatmaps.osu_session.as_deref())?;
            let semaphore = Arc::new(Semaphore::new(config.beatmaps.concurrency.max(1)));
            let cache_dir = config.paths.cache_dir.join("osz");
            let delivery_mode = config.beatmaps.delivery_mode.clone();
            let sources = config.beatmaps.sources.clone();
            let no_video = config.beatmaps.no_video;

            let succeeded = Arc::new(AtomicUsize::new(0));
            let failed = Arc::new(AtomicUsize::new(0));
            let mut tasks = Vec::new();

            for set in beatmaps {
                let sem = semaphore.clone();
                let client = http_client.clone();
                let c_dir = cache_dir.clone();
                let s_path = songs_path.clone();
                let d_mode = delivery_mode.clone();
                let srcs = sources.clone();
                let pb_clone = pb.clone();
                let succ = succeeded.clone();
                let fail = failed.clone();

                let task = tokio::spawn(async move {
                    let _permit = sem.acquire().await.unwrap();
                    pb_clone.set_message(format!("{} - {}", set.artist, set.title));

                    let download_res =
                        download_beatmap(&client, set.online_id, &srcs, no_video, &c_dir, 3).await;

                    match download_res {
                        Ok(osz_path) => {
                            if d_mode == "open" {
                                if let Err(e) = open_in_stable(&osz_path, None) {
                                    eprintln!("\nFailed to open osz {}: {}", set.online_id, e);
                                    fail.fetch_add(1, Ordering::SeqCst);
                                } else {
                                    succ.fetch_add(1, Ordering::SeqCst);
                                }
                            } else {
                                if let Err(e) = extract_to_songs(&osz_path, &s_path, set.online_id)
                                {
                                    eprintln!("\nFailed to extract osz {}: {}", set.online_id, e);
                                    fail.fetch_add(1, Ordering::SeqCst);
                                } else {
                                    succ.fetch_add(1, Ordering::SeqCst);
                                }
                            }
                        }
                        Err(err) => {
                            eprintln!("\nDownload error for set {}: {}", set.online_id, err);
                            fail.fetch_add(1, Ordering::SeqCst);
                        }
                    }
                    pb_clone.inc(1);
                });
                tasks.push(task);
            }

            for t in tasks {
                let _ = t.await;
            }

            pb.finish_with_message("Beatmap migration completed.");
            println!(
                "Beatmap migration finished. Succeeded: {}, Failed: {}",
                succeeded.load(Ordering::SeqCst),
                failed.load(Ordering::SeqCst)
            );
        }
        println!("--------------------------------------------------");
    }

    println!("All migration tasks finished.");
    Ok(())
}
