use lazer2stable_core::bridge;
use lazer2stable_core::models::{BeatmapSetDump, SkinDump};

#[test]
fn test_models_deserialization() {
    let beatmaps_json = r#"[
        {
            "online_id": 12345,
            "title": "Galaxy Collapse",
            "artist": "Kurokotei",
            "files": [
                { "filename": "audio.mp3", "hash": "a1b2c3d4" }
            ]
        }
    ]"#;
    let sets: Vec<BeatmapSetDump> =
        serde_json::from_str(beatmaps_json).expect("failed to parse beatmaps");
    assert_eq!(sets.len(), 1);
    assert_eq!(sets[0].online_id, 12345);
    assert_eq!(sets[0].title, "Galaxy Collapse");
    assert_eq!(sets[0].files.len(), 1);
    assert_eq!(sets[0].files[0].filename, "audio.mp3");

    let skins_json = r#"[
        {
            "id": "c6aa3ede-1685-4bb1-a5f5-b6896e59106a",
            "name": "Mrekk 3.0",
            "creator": "Flowfy",
            "files": [
                { "filename": "skin.ini", "hash": "99887766" }
            ]
        }
    ]"#;
    let skins: Vec<SkinDump> = serde_json::from_str(skins_json).expect("failed to parse skins");
    assert_eq!(skins.len(), 1);
    assert_eq!(skins[0].name, "Mrekk 3.0");
    assert_eq!(skins[0].files[0].filename, "skin.ini");
}

#[test]
fn test_fetch_from_local_realm() {
    let lazer_dir = lazer2stable_core::detector::detect_lazer_dir().expect("lazer dir not found");
    let realm_path = lazer2stable_core::detector::default_realm_path(&lazer_dir);
    if !realm_path.exists() {
        return;
    }

    let skins = bridge::fetch_skins(&realm_path, None).expect("failed to fetch skins");
    assert!(!skins.is_empty(), "expected custom skins to be found");

    let beatmaps = bridge::fetch_beatmaps(&realm_path, None).expect("failed to fetch beatmaps");
    assert!(!beatmaps.is_empty(), "expected beatmap sets to be found");
}
