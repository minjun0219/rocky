//! `rocky config init` — 없을 때만 만들고, 있으면 그대로.

use rocky_cli::config_cmd::init_config_file;

#[test]
fn init_writes_default_once_and_never_overwrites() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested").join("rocky.json");
    let first = init_config_file(&path).unwrap();
    assert!(first.starts_with("✓"), "{first}");
    let written = std::fs::read_to_string(&path).unwrap();
    let v: serde_json::Value = serde_json::from_str(&written).unwrap();
    assert_eq!(v["todo"]["expose"], "off");

    // 사용자가 고친 내용은 지키지 않으면 안 된다.
    std::fs::write(&path, "{\"todo\":{\"port\":9999}}").unwrap();
    let second = init_config_file(&path).unwrap();
    assert!(second.starts_with("="), "{second}");
    assert!(std::fs::read_to_string(&path).unwrap().contains("9999"));
}
