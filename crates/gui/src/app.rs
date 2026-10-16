//! Main application state and rendering loop for egui.

use crate::icons::*;
use crossbeam_channel::{Receiver, Sender};
use egui::{vec2, Color32, ProgressBar, RichText, Ui};
use lazer2stable_core::beatmaps::{extract_to_songs, installed_in_stable, open_in_stable};
use lazer2stable_core::bridge::{fetch_beatmaps, fetch_skins};
use lazer2stable_core::config::Config;
use lazer2stable_core::detector;
use lazer2stable_core::downloader::{create_http_client, download_beatmap};
use lazer2stable_core::models::{BeatmapSetDump, SkinDump};
use lazer2stable_core::skins::export_skin;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use tokio::sync::Semaphore;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveTab {
    Beatmaps,
    Skins,
    Settings,
}

#[derive(Debug, Clone)]
pub enum WorkerEvent {
    Status(String),
    Log(String),
    Progress { current: usize, total: usize, item_name: String },
    Done { succeeded: usize, failed: usize },
    Error(String),
    SkinsLoaded(Vec<SkinDump>),
    BeatmapsLoaded(Vec<BeatmapSetDump>),
}

pub struct Lazer2StableApp {
    config: Config,
    config_path: PathBuf,
    active_tab: ActiveTab,

    // Loaded library items
    skins: Vec<SkinDump>,
    selected_skins: HashSet<String>,
    beatmaps: Vec<BeatmapSetDump>,
    is_scanning: bool,

    // Migration execution state
    is_migrating: bool,
    cancel_flag: Arc<AtomicBool>,
    progress_current: usize,
    progress_total: usize,
    status_text: String,
    logs: Vec<String>,
    show_logs: bool,

    // Channels
    event_tx: Sender<WorkerEvent>,
    event_rx: Receiver<WorkerEvent>,
}

impl Lazer2StableApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Install SVG and image loaders for egui
        egui_extras::install_image_loaders(&cc.egui_ctx);

        let config_path = PathBuf::from("config.toml");
        let config = Config::load_or_default(&config_path).unwrap_or_default();

        let (event_tx, event_rx) = crossbeam_channel::unbounded();

        let mut app = Self {
            config,
            config_path,
            active_tab: ActiveTab::Beatmaps,
            skins: Vec::new(),
            selected_skins: HashSet::new(),
            beatmaps: Vec::new(),
            is_scanning: false,
            is_migrating: false,
            cancel_flag: Arc::new(AtomicBool::new(false)),
            progress_current: 0,
            progress_total: 0,
            status_text: "Ready to migrate".to_string(),
            logs: Vec::new(),
            show_logs: false,
            event_tx,
            event_rx,
        };

        // Trigger initial background scan
        app.trigger_scan();
        app
    }

    /// Triggers background scan of skins and beatmaps from client.realm.
    pub fn trigger_scan(&mut self) {
        if self.is_scanning || self.is_migrating {
            return;
        }

        self.is_scanning = true;
        self.status_text = "Scanning client.realm...".to_string();

        let tx = self.event_tx.clone();
        let config = self.config.clone();

        thread::spawn(move || {
            let lazer_dir = config.paths.lazer_dir.clone().or_else(detector::detect_lazer_dir);
            let Some(lazer_dir) = lazer_dir else {
                let _ = tx.send(WorkerEvent::Error("Could not locate osu!lazer directory".to_string()));
                return;
            };

            let realm_path = config
                .paths
                .realm_path
                .clone()
                .unwrap_or_else(|| detector::default_realm_path(&lazer_dir));

            if !realm_path.is_file() {
                let _ = tx.send(WorkerEvent::Error(format!("client.realm not found at {}", realm_path.display())));
                return;
            }

            let bridge_bin = config.paths.bridge_binary.as_deref();

            // 1. Fetch skins
            match fetch_skins(&realm_path, bridge_bin) {
                Ok(skins) => {
                    let _ = tx.send(WorkerEvent::Log(format!("Found {} custom skins in realm", skins.len())));
                    let _ = tx.send(WorkerEvent::SkinsLoaded(skins));
                }
                Err(e) => {
                    let _ = tx.send(WorkerEvent::Log(format!("Failed to scan skins: {}", e)));
                }
            }

            // 2. Fetch beatmaps
            match fetch_beatmaps(&realm_path, bridge_bin) {
                Ok(beatmaps) => {
                    let _ = tx.send(WorkerEvent::Log(format!("Found {} online beatmap sets in realm", beatmaps.len())));
                    let _ = tx.send(WorkerEvent::BeatmapsLoaded(beatmaps));
                }
                Err(e) => {
                    let _ = tx.send(WorkerEvent::Log(format!("Failed to scan beatmaps: {}", e)));
                }
            }

            let _ = tx.send(WorkerEvent::Status("Library scanned successfully".to_string()));
        });
    }

    /// Starts the migration process in a background thread.
    pub fn start_migration(&mut self) {
        if self.is_migrating {
            return;
        }

        self.is_migrating = true;
        self.cancel_flag.store(false, Ordering::SeqCst);
        self.progress_current = 0;
        self.status_text = "Starting migration...".to_string();

        let tx = self.event_tx.clone();
        let cancel = self.cancel_flag.clone();
        let config = self.config.clone();
        let selected_skin_ids = self.selected_skins.clone();
        let skins_to_export: Vec<SkinDump> = self
            .skins
            .iter()
            .filter(|s| selected_skin_ids.contains(&s.id))
            .cloned()
            .collect();
        let beatmaps_to_export = self.beatmaps.clone();

        thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build();

            let Ok(rt) = rt else {
                let _ = tx.send(WorkerEvent::Error("Failed to initialize async runtime".to_string()));
                return;
            };

            rt.block_on(async {
                let lazer_dir = config.paths.lazer_dir.clone().or_else(detector::detect_lazer_dir);
                let stable_dir = config.paths.stable_dir.clone().or_else(detector::detect_stable_dir);

                let Some(lazer_dir) = lazer_dir else {
                    let _ = tx.send(WorkerEvent::Error("osu!lazer directory not found".to_string()));
                    return;
                };

                let files_dir = lazer_dir.join("files");
                let stable_dir_ref = stable_dir.as_deref();

                let songs_dir = config.paths.songs_dir.clone().or_else(|| {
                    stable_dir_ref.map(detector::default_songs_dir)
                });

                let skins_dir = config.paths.skins_dir.clone().or_else(|| {
                    stable_dir_ref.map(detector::default_skins_dir)
                });

                let mut total_succeeded = 0;
                let mut total_failed = 0;

                // 1. Migrate Skins
                if config.skins.enabled && !skins_to_export.is_empty() {
                    let _ = tx.send(WorkerEvent::Status("Migrating skins...".to_string()));
                    let total_skins = skins_to_export.len();
                    let osk_export_dir = config.paths.cache_dir.join("skins");

                    for (idx, skin) in skins_to_export.into_iter().enumerate() {
                        if cancel.load(Ordering::SeqCst) {
                            let _ = tx.send(WorkerEvent::Log("Migration cancelled by user.".to_string()));
                            break;
                        }

                        let _ = tx.send(WorkerEvent::Progress {
                            current: idx + 1,
                            total: total_skins,
                            item_name: format!("Skin: {}", skin.name),
                        });

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

                        match export_skin(&skin, &files_dir, osk_path.as_deref(), target_skins) {
                            Ok(()) => {
                                total_succeeded += 1;
                                let _ = tx.send(WorkerEvent::Log(format!("Exported skin: {}", skin.name)));
                            }
                            Err(e) => {
                                total_failed += 1;
                                let _ = tx.send(WorkerEvent::Log(format!("Error exporting skin {}: {}", skin.name, e)));
                            }
                        }
                    }
                }

                // 2. Migrate Beatmaps
                if config.beatmaps.enabled && !beatmaps_to_export.is_empty() && !cancel.load(Ordering::SeqCst) {
                    let _ = tx.send(WorkerEvent::Status("Migrating beatmaps...".to_string()));

                    let mut beatmaps = beatmaps_to_export;
                    if config.beatmaps.skip_existing {
                        if let Some(ref s_dir) = songs_dir {
                            let existing = installed_in_stable(s_dir);
                            beatmaps.retain(|b| !existing.contains(&b.online_id));
                        }
                    }

                    let total_maps = beatmaps.len();
                    let _ = tx.send(WorkerEvent::Log(format!("Beatmaps to download/extract: {}", total_maps)));

                    if let Some(ref s_path) = songs_dir {
                        let http_client = match create_http_client(config.beatmaps.osu_session.as_deref()) {
                            Ok(c) => c,
                            Err(e) => {
                                let _ = tx.send(WorkerEvent::Error(format!("Failed to build HTTP client: {}", e)));
                                return;
                            }
                        };

                        let semaphore = Arc::new(Semaphore::new(config.beatmaps.concurrency.max(1)));
                        let cache_dir = config.paths.cache_dir.join("osz");
                        let delivery_mode = config.beatmaps.delivery_mode.clone();
                        let sources = config.beatmaps.sources.clone();
                        let no_video = config.beatmaps.no_video;

                        for (idx, set) in beatmaps.into_iter().enumerate() {
                            if cancel.load(Ordering::SeqCst) {
                                break;
                            }

                            let _ = tx.send(WorkerEvent::Progress {
                                current: idx + 1,
                                total: total_maps,
                                item_name: format!("{} - {}", set.artist, set.title),
                            });

                            let _permit = semaphore.acquire().await.unwrap();
                            let dl_res = download_beatmap(
                                &http_client,
                                set.online_id,
                                &sources,
                                no_video,
                                &cache_dir,
                                3,
                            ).await;

                            match dl_res {
                                Ok(osz_path) => {
                                    if delivery_mode == "open" {
                                        if let Err(e) = open_in_stable(&osz_path, None) {
                                            total_failed += 1;
                                            let _ = tx.send(WorkerEvent::Log(format!("Failed to open osz {}: {}", set.online_id, e)));
                                        } else {
                                            total_succeeded += 1;
                                        }
                                    } else {
                                        if let Err(e) = extract_to_songs(&osz_path, s_path, set.online_id) {
                                            total_failed += 1;
                                            let _ = tx.send(WorkerEvent::Log(format!("Failed to extract osz {}: {}", set.online_id, e)));
                                        } else {
                                            total_succeeded += 1;
                                        }
                                    }
                                }
                                Err(err) => {
                                    total_failed += 1;
                                    let _ = tx.send(WorkerEvent::Log(format!("Download failed for set {}: {}", set.online_id, err)));
                                }
                            }
                        }
                    } else {
                        let _ = tx.send(WorkerEvent::Error("Songs directory not specified".to_string()));
                    }
                }

                let _ = tx.send(WorkerEvent::Done {
                    succeeded: total_succeeded,
                    failed: total_failed,
                });
            });
        });
    }

    /// Cancels ongoing migration.
    pub fn cancel_migration(&mut self) {
        if self.is_migrating {
            self.cancel_flag.store(true, Ordering::SeqCst);
            self.status_text = "Cancelling...".to_string();
        }
    }

    /// Processes incoming worker events.
    fn drain_events(&mut self) {
        while let Ok(event) = self.event_rx.try_recv() {
            match event {
                WorkerEvent::Status(msg) => {
                    self.status_text = msg;
                }
                WorkerEvent::Log(msg) => {
                    self.logs.push(msg);
                }
                WorkerEvent::Progress { current, total, item_name } => {
                    self.progress_current = current;
                    self.progress_total = total;
                    self.status_text = format!("{}/{} - {}", current, total, item_name);
                }
                WorkerEvent::Done { succeeded, failed } => {
                    self.is_migrating = false;
                    self.status_text = format!("Finished! Succeeded: {}, Failed: {}", succeeded, failed);
                    self.logs.push(format!("Migration finished. Succeeded: {}, Failed: {}", succeeded, failed));
                }
                WorkerEvent::Error(err) => {
                    self.is_migrating = false;
                    self.is_scanning = false;
                    self.status_text = format!("Error: {}", err);
                    self.logs.push(format!("ERROR: {}", err));
                }
                WorkerEvent::SkinsLoaded(skins) => {
                    self.selected_skins = skins.iter().map(|s| s.id.clone()).collect();
                    self.skins = skins;
                    self.is_scanning = false;
                }
                WorkerEvent::BeatmapsLoaded(beatmaps) => {
                    self.beatmaps = beatmaps;
                    self.is_scanning = false;
                }
            }
        }
    }
}

impl eframe::App for Lazer2StableApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_events();

        // Dark theme tuning
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = Color32::from_rgb(24, 24, 31);
        visuals.window_fill = Color32::from_rgb(28, 28, 36);
        visuals.override_text_color = Some(Color32::from_rgb(230, 230, 240));
        ctx.set_visuals(visuals);

        // Request continuous repaint if active work is ongoing
        if self.is_migrating || self.is_scanning {
            ctx.request_repaint();
        }

        egui::TopBottomPanel::top("header_panel")
            .frame(egui::Frame::side_top_panel(&ctx.style()).fill(Color32::from_rgb(20, 20, 26)))
            .show(ctx, |ui| {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.heading(RichText::new("lazer2stable").strong().color(Color32::from_rgb(255, 102, 170)));
                    ui.label(RichText::new("v2.0").weak());

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if self.is_scanning {
                            ui.spinner();
                            ui.label("Scanning...");
                        } else {
                            if ui.button("Rescan Library").clicked() {
                                self.trigger_scan();
                            }
                        }
                    });
                });
                ui.add_space(8.0);

                // Tab Buttons with clean SVG icons
                ui.horizontal(|ui| {
                    let beatmaps_label = format!("Beatmaps ({})", self.beatmaps.len());
                    let skins_label = format!("Skins ({})", self.skins.len());

                    tab_button(ui, &mut self.active_tab, ActiveTab::Beatmaps, ICON_BEATMAPS, &beatmaps_label);
                    tab_button(ui, &mut self.active_tab, ActiveTab::Skins, ICON_SKINS, &skins_label);
                    tab_button(ui, &mut self.active_tab, ActiveTab::Settings, ICON_SETTINGS, "Settings");
                });
                ui.add_space(4.0);
            });

        // Bottom status and action bar
        egui::TopBottomPanel::bottom("bottom_bar")
            .frame(egui::Frame::side_top_panel(&ctx.style()).fill(Color32::from_rgb(20, 20, 26)))
            .show(ctx, |ui| {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if self.is_migrating {
                        if ui.add(
                            egui::Button::new(RichText::new("Cancel").color(Color32::from_rgb(255, 80, 80)).strong().size(13.0))
                                .min_size(vec2(130.0, 32.0)),
                        ).clicked() {
                            self.cancel_migration();
                        }
                    } else {
                        let btn = ui.add(
                            egui::Button::new(RichText::new("Start Migration").strong().size(13.0))
                                .fill(Color32::from_rgb(255, 102, 170))
                                .min_size(vec2(130.0, 32.0)),
                        );
                        if btn.clicked() {
                            self.start_migration();
                        }
                    }

                    let right_reserved = 95.0;
                    let center_width = (ui.available_width() - right_reserved).max(80.0);

                    ui.allocate_ui_with_layout(
                        vec2(center_width, 32.0),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.label(RichText::new(&self.status_text).strong().size(12.0));
                            let fraction = if self.progress_total > 0 {
                                self.progress_current as f32 / self.progress_total as f32
                            } else {
                                0.0
                            };
                            ui.add(ProgressBar::new(fraction).show_percentage());
                        },
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let log_btn_text = if self.show_logs { "Hide Logs" } else { "Show Logs" };
                        if ui.button(log_btn_text).clicked() {
                            self.show_logs = !self.show_logs;
                        }
                    });
                });
                ui.add_space(8.0);

                if self.show_logs {
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .max_height(140.0)
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            for line in &self.logs {
                                ui.label(RichText::new(line).monospace().size(11.0));
                            }
                        });
                    ui.add_space(4.0);
                }
            });

        // Central tab content with responsive vertical scrolling
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add_space(8.0);
                    match self.active_tab {
                        ActiveTab::Beatmaps => self.render_beatmaps_tab(ui),
                        ActiveTab::Skins => self.render_skins_tab(ui),
                        ActiveTab::Settings => self.render_settings_tab(ui),
                    }
                });
        });
    }
}

fn tab_button(ui: &mut Ui, active: &mut ActiveTab, tab: ActiveTab, icon: egui::ImageSource<'_>, label: &str) {
    let is_selected = *active == tab;
    let text = if is_selected {
        RichText::new(label).strong().color(Color32::from_rgb(255, 102, 170))
    } else {
        RichText::new(label).color(Color32::from_rgb(170, 170, 185))
    };

    let btn = ui.add(
        egui::Button::image_and_text(
            egui::Image::new(icon).fit_to_exact_size(vec2(16.0, 16.0)),
            text,
        )
        .min_size(vec2(120.0, 28.0)),
    );
    if btn.clicked() {
        *active = tab;
    }
}

impl Lazer2StableApp {
    fn render_beatmaps_tab(&mut self, ui: &mut Ui) {
        ui.heading("Beatmaps Migration");
        ui.add_space(6.0);

        ui.checkbox(&mut self.config.beatmaps.enabled, "Enable beatmap migration");
        ui.add_space(8.0);

        // Dim and disable settings when beatmap migration is toggled off
        ui.add_enabled_ui(self.config.beatmaps.enabled, |ui| {
            ui.group(|ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new("Import Method").strong());
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    ui.radio_value(&mut self.config.beatmaps.source_mode, "network".to_string(), "Download via online mirrors (recommended)");
                    ui.radio_value(&mut self.config.beatmaps.source_mode, "local".to_string(), "Assemble offline from lazer cache");
                });
                ui.add_space(8.0);

                ui.label(RichText::new("Delivery Target").strong());
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    ui.radio_value(&mut self.config.beatmaps.delivery_mode, "extract".to_string(), "Extract directly to Songs/ folder (fast)");
                    ui.radio_value(&mut self.config.beatmaps.delivery_mode, "open".to_string(), "Launch each .osz with osu!.exe");
                });
            });

            ui.add_space(10.0);

            ui.group(|ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new("Optimization Filters").strong());
                ui.add_space(4.0);
                ui.checkbox(&mut self.config.beatmaps.skip_existing, "Skip beatmaps already installed in stable Songs/");
                ui.checkbox(&mut self.config.beatmaps.no_video, "Exclude video tracks (saves bandwidth and disk space)");
            });

            ui.add_space(10.0);
            ui.label(format!("Online beatmap sets found in lazer: {}", self.beatmaps.len()));
        });
    }

    fn render_skins_tab(&mut self, ui: &mut Ui) {
        ui.heading("Skins Migration");
        ui.add_space(6.0);

        ui.checkbox(&mut self.config.skins.enabled, "Enable custom skins migration");
        ui.add_space(8.0);

        // Dim and disable settings when skin migration is toggled off
        ui.add_enabled_ui(self.config.skins.enabled, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.checkbox(&mut self.config.skins.unpack_to_stable, "Unpack directly to stable Skins/ folder");
                ui.checkbox(&mut self.config.skins.export_as_osk, "Save packaged .osk archive to exports");
            });

            ui.add_space(8.0);

            ui.horizontal_wrapped(|ui| {
                if ui.button("Select All").clicked() {
                    self.selected_skins = self.skins.iter().map(|s| s.id.clone()).collect();
                }
                if ui.button("Deselect All").clicked() {
                    self.selected_skins.clear();
                }
                ui.label(format!("Selected {} of {} skin(s)", self.selected_skins.len(), self.skins.len()));
            });

            ui.separator();

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .max_height(ui.available_height().max(180.0))
                .show(ui, |ui| {
                    if self.skins.is_empty() {
                        ui.label("No custom skins detected in client.realm.");
                    } else {
                        for skin in &self.skins {
                            let mut is_checked = self.selected_skins.contains(&skin.id);
                            ui.horizontal_wrapped(|ui| {
                                if ui.checkbox(&mut is_checked, "").changed() {
                                    if is_checked {
                                        self.selected_skins.insert(skin.id.clone());
                                    } else {
                                        self.selected_skins.remove(&skin.id);
                                    }
                                }
                                ui.label(RichText::new(&skin.name).strong());
                                ui.label(RichText::new(format!("({} files)", skin.files.len())).weak());
                                if !skin.creator.is_empty() {
                                    ui.label(RichText::new(format!("by {}", skin.creator)).italics().weak());
                                }
                            });
                        }
                    }
                });
        });
    }

    fn render_settings_tab(&mut self, ui: &mut Ui) {
        ui.heading("Settings & Path Configuration");
        ui.add_space(8.0);

        render_path_field(
            ui,
            "Lazer Directory:",
            &mut self.config.paths.lazer_dir,
            "Root folder of osu!lazer containing client.realm and files/",
        );
        ui.add_space(6.0);

        render_path_field(
            ui,
            "Stable Directory:",
            &mut self.config.paths.stable_dir,
            "Root folder of osu!stable containing osu!.exe",
        );
        ui.add_space(6.0);

        render_path_field(
            ui,
            "Songs Folder:",
            &mut self.config.paths.songs_dir,
            "Songs directory inside osu!stable",
        );
        ui.add_space(6.0);

        render_path_field(
            ui,
            "Skins Folder:",
            &mut self.config.paths.skins_dir,
            "Skins directory inside osu!stable",
        );

        ui.add_space(14.0);
        ui.separator();
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            ui.label("Network Concurrency:");
            ui.add(egui::Slider::new(&mut self.config.beatmaps.concurrency, 1..=16));
        });

        ui.add_space(14.0);

        if ui.button(RichText::new("Save Configuration").strong()).clicked() {
            if let Err(e) = self.config.save(&self.config_path) {
                self.status_text = format!("Failed to save config: {}", e);
            } else {
                self.status_text = "Configuration saved to config.toml".to_string();
            }
        }
    }
}

fn render_path_field(ui: &mut Ui, label: &str, path_opt: &mut Option<PathBuf>, hint: &str) {
    ui.label(RichText::new(label).strong());
    ui.horizontal(|ui| {
        let btn_width = 85.0;
        let input_width = (ui.available_width() - btn_width - 8.0).max(120.0);

        let mut path_str = path_opt
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        let resp = ui.add_sized(
            vec2(input_width, 24.0),
            egui::TextEdit::singleline(&mut path_str).hint_text(hint),
        );
        if resp.changed() {
            *path_opt = if path_str.trim().is_empty() {
                None
            } else {
                Some(PathBuf::from(path_str.trim()))
            };
        }

        if ui.add_sized(vec2(btn_width, 24.0), egui::Button::new("Browse...")).clicked() {
            if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                *path_opt = Some(folder);
            }
        }
    });
}
