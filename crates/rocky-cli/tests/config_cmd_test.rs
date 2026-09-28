//! `rocky config init` — 없을 때만 만들고, 있으면 그대로.

use rocky_cli::config_cmd::{init_config_file, link_cli};

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

#[test]
fn link_creates_replaces_and_refuses_foreign_files() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("bin");
    let target = dir.path().join("current").join("rocky");
    let first = link_cli(&bin, &target).unwrap();
    assert!(first.starts_with("✓"), "{first}");
    assert_eq!(std::fs::read_link(bin.join("rocky")).unwrap(), target);
    // 같은 링크면 no-op.
    assert!(link_cli(&bin, &target).unwrap().starts_with("="));
    // 다른 곳을 가리키던 링크는 갈아 끼운다.
    let other = dir.path().join("elsewhere");
    std::fs::remove_file(bin.join("rocky")).unwrap();
    std::os::unix::fs::symlink(&other, bin.join("rocky")).unwrap();
    assert!(link_cli(&bin, &target).unwrap().starts_with("✓"));
    assert_eq!(std::fs::read_link(bin.join("rocky")).unwrap(), target);
    // 남의 실제 파일은 거절.
    std::fs::remove_file(bin.join("rocky")).unwrap();
    std::fs::write(bin.join("rocky"), "theirs").unwrap();
    assert!(link_cli(&bin, &target).is_err());
    assert_eq!(
        std::fs::read_to_string(bin.join("rocky")).unwrap(),
        "theirs"
    );
}
