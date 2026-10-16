use lazer2stable_core::models::{NamedFile, SkinDump};
use lazer2stable_core::skins::{export_skin, locate_lazer_file, sanitize_name};
use std::fs;

#[test]
fn test_locate_lazer_file() {
    let temp = tempfile::tempdir().unwrap();
    let files_dir = temp.path().join("files");

    let hash = "ab1234567890abcdef";
    let expected_path = files_dir.join("a").join("ab").join(hash);
    fs::create_dir_all(expected_path.parent().unwrap()).unwrap();
    fs::write(&expected_path, b"test file content").unwrap();

    let found = locate_lazer_file(&files_dir, hash);
    assert_eq!(found, Some(expected_path));

    let not_found = locate_lazer_file(&files_dir, "nonexistent");
    assert_eq!(not_found, None);
}

#[test]
fn test_sanitize_name() {
    assert_eq!(sanitize_name("Skin: \"Best\" / <osu!>"), "Skin_ _Best_ _ _osu!_");
    assert_eq!(sanitize_name("Normal Skin"), "Normal Skin");
}

#[test]
fn test_export_skin_osk_and_unpack() {
    let temp = tempfile::tempdir().unwrap();
    let lazer_files = temp.path().join("files");
    let out_osk_dir = temp.path().join("exports");
    let stable_skins_dir = temp.path().join("stable_skins");

    let hash1 = "1111111111111111";
    let file1_path = lazer_files.join("1").join("11").join(hash1);
    fs::create_dir_all(file1_path.parent().unwrap()).unwrap();
    fs::write(&file1_path, b"[General]\nName: TestSkin\n").unwrap();

    let hash2 = "2222222222222222";
    let file2_path = lazer_files.join("2").join("22").join(hash2);
    fs::create_dir_all(file2_path.parent().unwrap()).unwrap();
    fs::write(&file2_path, b"fake png data").unwrap();

    let skin = SkinDump {
        id: "skin-123".to_string(),
        name: "Test Skin".to_string(),
        creator: "Maksym".to_string(),
        files: vec![
            NamedFile {
                filename: "skin.ini".to_string(),
                hash: hash1.to_string(),
            },
            NamedFile {
                filename: "cursor.png".to_string(),
                hash: hash2.to_string(),
            },
        ],
    };

    let osk_path = out_osk_dir.join("Test Skin.osk");
    export_skin(
        &skin,
        &lazer_files,
        Some(&osk_path),
        Some(&stable_skins_dir),
    )
    .expect("export_skin failed");

    // Check .osk archive exists and can be read as zip
    assert!(osk_path.exists());
    let file = fs::File::open(&osk_path).unwrap();
    let zip = zip::ZipArchive::new(file).unwrap();
    assert_eq!(zip.len(), 2);

    // Check unpacked folder in stable skins directory
    let unpacked_folder = stable_skins_dir.join("Test Skin");
    assert!(unpacked_folder.exists());
    assert_eq!(
        fs::read_to_string(unpacked_folder.join("skin.ini")).unwrap(),
        "[General]\nName: TestSkin\n"
    );
    assert_eq!(
        fs::read(unpacked_folder.join("cursor.png")).unwrap(),
        b"fake png data"
    );
}
