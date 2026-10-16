//! HTTP download manager and mirror rotation for beatmap .osz files.

use anyhow::{bail, Context, Result};
use reqwest::header::RETRY_AFTER;
use reqwest::{Client, StatusCode};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::time::sleep;
use tracing::{debug, info, warn};

/// Builds the download URL for a given source and beatmap set ID.
pub fn build_download_url(source: &str, id: i64, no_video: bool) -> Option<String> {
    let lower = source.to_lowercase();
    match lower.as_str() {
        "catboy" => {
            if no_video {
                Some(format!("https://catboy.best/d/{}?noVideo=1", id))
            } else {
                Some(format!("https://catboy.best/d/{}", id))
            }
        }
        "nerinyan" => {
            if no_video {
                Some(format!("https://api.nerinyan.moe/d/{}?noVideo=1", id))
            } else {
                Some(format!("https://api.nerinyan.moe/d/{}", id))
            }
        }
        "osudirect" => {
            if no_video {
                Some(format!("https://api.osu.direct/d/{}?noVideo=1", id))
            } else {
                Some(format!("https://api.osu.direct/d/{}", id))
            }
        }
        "official" => {
            if no_video {
                Some(format!("https://osu.ppy.sh/beatmapsets/{}/download?noVideo=1", id))
            } else {
                Some(format!("https://osu.ppy.sh/beatmapsets/{}/download", id))
            }
        }
        custom if custom.contains("{id}") => {
            Some(custom.replace("{id}", &id.to_string()))
        }
        _ => None,
    }
}

/// Creates a tuned HTTP client with standard timeouts and headers.
pub fn create_http_client(osu_session: Option<&str>) -> Result<Client> {
    let mut builder = Client::builder()
        .user_agent("lazer2stable/2.0 (Rust; +https://github.com/ppy/osu)")
        .timeout(Duration::from_secs(60))
        .connect_timeout(Duration::from_secs(15));

    if let Some(session) = osu_session {
        if !session.trim().is_empty() {
            let cookie = format!("osu_session={}; Domain=osu.ppy.sh; Path=/; Secure; HttpOnly", session.trim());
            let url = "https://osu.ppy.sh".parse::<reqwest::Url>()?;
            let jar = reqwest::cookie::Jar::default();
            jar.add_cookie_str(&cookie, &url);
            builder = builder.cookie_provider(std::sync::Arc::new(jar));
        }
    }

    builder.build().context("Failed to build HTTP client")
}

/// Downloads a beatmap set by ID, rotating through mirror sources and handling rate limits.
pub async fn download_beatmap(
    client: &Client,
    id: i64,
    sources: &[String],
    no_video: bool,
    out_cache_dir: &Path,
    max_retries: usize,
) -> Result<PathBuf> {
    fs::create_dir_all(out_cache_dir)?;
    let target_osz = out_cache_dir.join(format!("{}.osz", id));

    // If already downloaded and valid, return immediately
    if target_osz.is_file() && target_osz.metadata()?.len() > 1024 {
        debug!("Beatmap {} already exists in cache", id);
        return Ok(target_osz);
    }

    let temp_osz = out_cache_dir.join(format!("{}.osz.part", id));
    let mut last_errors = Vec::new();

    for attempt in 0..max_retries {
        for source_name in sources {
            let Some(url) = build_download_url(source_name, id, no_video) else {
                continue;
            };

            debug!("Attempting download of map {} from {} (attempt {})", id, source_name, attempt + 1);
            match client.get(&url).send().await {
                Ok(response) => {
                    let status = response.status();
                    if status == StatusCode::TOO_MANY_REQUESTS {
                        let retry_after_secs = response
                            .headers()
                            .get(RETRY_AFTER)
                            .and_then(|val| val.to_str().ok())
                            .and_then(|val| val.parse::<u64>().ok())
                            .unwrap_or(5);

                        let wait_time = Duration::from_secs(retry_after_secs.min(60));
                        warn!("Rate limited (429) on {}. Sleeping for {}s", source_name, wait_time.as_secs());
                        sleep(wait_time).await;
                        continue;
                    }

                    if status.is_success() {
                        let mut file = File::create(&temp_osz)?;
                        let bytes = response.bytes().await?;
                        if bytes.len() < 1024 {
                            last_errors.push(format!("{}: payload too small ({} bytes)", source_name, bytes.len()));
                            continue;
                        }
                        file.write_all(&bytes)?;
                        file.flush()?;

                        // Rename from partial to final
                        fs::rename(&temp_osz, &target_osz)?;
                        info!("Successfully downloaded beatmap {} from {}", id, source_name);
                        return Ok(target_osz);
                    } else if status == StatusCode::NOT_FOUND {
                        last_errors.push(format!("{}: 404 Not Found", source_name));
                    } else {
                        last_errors.push(format!("{}: HTTP {}", source_name, status));
                    }
                }
                Err(err) => {
                    last_errors.push(format!("{}: {}", source_name, err));
                }
            }
        }

        // Backoff between retry rounds
        if attempt + 1 < max_retries {
            sleep(Duration::from_millis(500 * (1 << attempt))).await;
        }
    }

    if temp_osz.exists() {
        let _ = fs::remove_file(&temp_osz);
    }

    bail!("Failed to download beatmap {}: {}", id, last_errors.join("; "))
}
