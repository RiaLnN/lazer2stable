//! Embedded SVG icon definitions.

#![allow(dead_code)]

use egui::ImageSource;

pub const ICON_BEATMAPS: ImageSource<'static> = egui::include_image!("../assets/beatmaps.svg");
pub const ICON_SKINS: ImageSource<'static> = egui::include_image!("../assets/skins.svg");
pub const ICON_SETTINGS: ImageSource<'static> = egui::include_image!("../assets/settings.svg");
pub const ICON_FOLDER: ImageSource<'static> = egui::include_image!("../assets/folder.svg");
pub const ICON_CHECK: ImageSource<'static> = egui::include_image!("../assets/check.svg");
pub const ICON_ALERT: ImageSource<'static> = egui::include_image!("../assets/alert.svg");
