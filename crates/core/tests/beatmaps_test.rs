use lazer2stable_core::beatmaps::{extract_to_songs, installed_in_stable};
use lazer2stable_core::downloader::build_download_url;
use std::fs::{self, File};
use std::io::Write;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

#[test]
fn test_build_download_url() {
    let catboy = build_download_url("catboy", 12345, false).unwrap();
    assert_eq!(catboy, "https://catboy.best/d/12345");

    let catboy_novideo = build_download_url("catboy", 12345, true).unwrap();
    assert_eq!(catboy_novideo, "https://catboy.best/d/12345?noVideo=1");

    let nerinyan = build_download_url("nerinyan", 12345, false).unwrap();
    assert_eq!(nerinyan, "https://api.nerinyan.moe/d/12345");

    let osudirect = build_download_url("osudirect", 12345, false).unwrap();
    assert_eq!(osudirect, "https://api.osu.direct/d/12345");
}

#[test]
fn test_installed_in_stable_scanner() {
    let temp = tempfile::tempdir().unwrap();
    let songs_dir = temp.path().join("Songs");
    fs::create_dir_all(&songs_dir).unwrap();

    fs::create_dir(songs_dir.join("12345 Kurokotei - Galaxy Collapse")).unwrap();
    fs::create_dir(songs_dir.join("67890")).unwrap();
    fs::create_dir(songs_dir.join("not_an_id_folder")).unwrap();

    let installed = installed_in_stable(&songs_dir);
    assert!(installed.contains(&12345));
    assert!(installed.contains(&67890));
    assert_eq!(installed.len(), 2);
}

#[test]
fn test_extract_to_songs() {
    let temp = tempfile::tempdir().unwrap();
    let songs_dir = temp.path().join("Songs");
    let osz_path = temp.path().join("12345.osz");

    // Create a mock .osz
    let file = File::create(&osz_path).unwrap();
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default();
    zip.start_file("song.osu", options).unwrap();
    zip.write_all(b"osu file format v14").unwrap();
    zip.finish().unwrap();

    extract_to_songs(&osz_path, &songs_dir, 12345).expect("extraction failed");

    let target_folder = songs_dir.join("12345");
    assert!(target_folder.exists());
    assert_eq!(
        fs::read_to_string(target_folder.join("song.osu")).unwrap(),
        "osu file format v14"
    );
}
