use lazer2stable_core::config::Config;

#[test]
fn test_default_config_serialization() {
    let config = Config::default();
    let toml_str = toml::to_string(&config).expect("failed to serialize");
    let deserialized: Config = toml::from_str(&toml_str).expect("failed to deserialize");
    assert_eq!(
        config.beatmaps.concurrency,
        deserialized.beatmaps.concurrency
    );
    assert_eq!(config.beatmaps.source_mode, "network");
    assert!(config.beatmaps.enabled);
    assert!(config.skins.enabled);
}

#[test]
fn test_config_save_and_load() {
    let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
    let config_path = temp_dir.path().join("config.toml");

    let mut config = Config::default();
    config.beatmaps.concurrency = 8;
    config.save(&config_path).expect("failed to save config");

    let loaded = Config::load_or_default(&config_path).expect("failed to load config");
    assert_eq!(loaded.beatmaps.concurrency, 8);
}

#[test]
fn test_detector_lazer_dir() {
    let lazer_dir = lazer2stable_core::detector::detect_lazer_dir();
    assert!(
        lazer_dir.is_some(),
        "lazer installation should be detected on this machine"
    );
    let realm = lazer2stable_core::detector::default_realm_path(&lazer_dir.unwrap());
    assert!(realm.exists(), "detected client.realm file should exist");
}
