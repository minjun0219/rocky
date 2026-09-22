//! TS `src/core/worklog.test.ts` 포팅. 프로젝트 키는 TS 와 바이트 동일해야 한다 —
//! 실제 앵커 디렉터리 이름(`rocky-4745b950`)을 골든으로 박아 둔다.

use std::path::{Path, PathBuf};

use rocky_todo_core::worklog::*;

const PAGE: &str = "1234abcd1234abcd1234abcd1234abcd";
const PAGE_DASHED: &str = "1234abcd-1234-abcd-1234-abcd1234abcd";
const OTHER_PAGE: &str = "abcd1234abcd1234abcd1234abcd1234";

fn temp_worklog() -> (tempfile::TempDir, Worklog) {
    let dir = tempfile::tempdir().unwrap();
    let worklog = Worklog::new(WorklogOptions {
        base_dir: Some(dir.path().to_string_lossy().to_string()),
        project_key: Some("t-00000000".into()),
        ..Default::default()
    });
    (dir, worklog)
}

fn append(w: &Worklog, content: &str) -> WorklogEntry {
    w.append(&WorklogAppendInput {
        content: content.into(),
        ..Default::default()
    })
    .unwrap()
}

fn contents(entries: &[WorklogEntry]) -> Vec<&str> {
    entries.iter().map(|e| e.content.as_str()).collect()
}

#[test]
fn append_writes_normalized_fields() {
    let (_d, w) = temp_worklog();
    let entry = w
        .append(&WorklogAppendInput {
            content: "  decided to use Bun  ".into(),
            kind: Some("decision".into()),
            tags: Some(vec![" notion ".into(), "".into(), "infra".into()]),
            page_id: Some(PAGE.into()),
        })
        .unwrap();
    assert_eq!(entry.content, "decided to use Bun");
    assert_eq!(entry.kind, "decision");
    assert_eq!(entry.tags, vec!["notion", "infra"]);
    assert_eq!(entry.page_id.as_deref(), Some(PAGE_DASHED));
    let (ms, hex) = entry.id.split_once('-').unwrap();
    assert!(ms.chars().all(|c| c.is_ascii_digit()));
    assert_eq!(hex.len(), 6);
    assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
    assert!(entry.timestamp.ends_with('Z'));
    assert_eq!(entry.timestamp.len(), "2026-01-01T00:00:00.000Z".len());
}

#[test]
fn append_defaults_kind_note_and_empty_tags() {
    let (_d, w) = temp_worklog();
    let entry = append(&w, "blocker");
    assert_eq!(entry.kind, "note");
    assert!(entry.tags.is_empty());
    assert!(entry.page_id.is_none());
}

#[test]
fn append_rejects_empty_content_and_bad_page_id() {
    let (_d, w) = temp_worklog();
    let err = w
        .append(&WorklogAppendInput {
            content: "   ".into(),
            ..Default::default()
        })
        .unwrap_err();
    assert!(err.contains("non-empty"));
    let err = w
        .append(&WorklogAppendInput {
            content: "x".into(),
            page_id: Some("not-a-page".into()),
            ..Default::default()
        })
        .unwrap_err();
    assert!(err.contains("Notion page id"));
}

#[test]
fn file_line_matches_ts_shape() {
    // 파일에 찍히는 JSON 의 필드 순서·이름이 TS `JSON.stringify` 와 같아야 옛 파일과 섞인다.
    let (d, w) = temp_worklog();
    w.append(&WorklogAppendInput {
        content: "a".into(),
        kind: Some("decision".into()),
        tags: Some(vec!["x".into()]),
        page_id: Some(PAGE.into()),
    })
    .unwrap();
    let raw = std::fs::read_to_string(d.path().join(WORKLOG_FILE)).unwrap();
    let line = raw.trim();
    assert!(line.starts_with("{\"id\":\""));
    let value = serde_json::from_str::<serde_json::Value>(line).unwrap();
    let keys: Vec<&str> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        vec!["id", "timestamp", "kind", "content", "tags", "pageId"]
    );
    // pageId 없으면 키 자체가 없다.
    append(&w, "b");
    let last = std::fs::read_to_string(d.path().join(WORKLOG_FILE)).unwrap();
    assert!(!last.lines().last().unwrap().contains("pageId"));
}

#[test]
fn read_returns_most_recent_first_up_to_limit() {
    let (_d, w) = temp_worklog();
    for i in 0..25 {
        append(&w, &format!("entry {i}"));
    }
    let recent = w.read(&WorklogReadOptions::default()).unwrap();
    assert_eq!(recent.len(), 20);
    assert_eq!(recent[0].content, "entry 24");
    assert_eq!(recent[19].content, "entry 5");
}

#[test]
fn read_filters_by_kind_tag_page_id() {
    let (_d, w) = temp_worklog();
    let mk = |content: &str, kind: &str, tags: &[&str], page: Option<&str>| {
        w.append(&WorklogAppendInput {
            content: content.into(),
            kind: Some(kind.into()),
            tags: Some(tags.iter().map(|t| t.to_string()).collect()),
            page_id: page.map(str::to_string),
        })
        .unwrap();
    };
    mk("a", "decision", &["api", "review"], Some(PAGE));
    mk("b", "blocker", &["review"], Some(OTHER_PAGE));
    mk("c", "decision", &["api"], Some(PAGE_DASHED));
    let by_kind = w
        .read(&WorklogReadOptions {
            kind: Some("decision".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(contents(&by_kind), vec!["c", "a"]);
    let by_tag = w
        .read(&WorklogReadOptions {
            tag: Some("api".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(contents(&by_tag), vec!["c", "a"]);
    let by_page = w
        .read(&WorklogReadOptions {
            page_id: Some(format!("https://www.notion.so/team/Title-{PAGE}")),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(contents(&by_page), vec!["c", "a"]);
    assert!(by_page
        .iter()
        .all(|e| e.page_id.as_deref() == Some(PAGE_DASHED)));
}

#[test]
fn read_filters_by_since_strictly_after() {
    let (_d, w) = temp_worklog();
    let before = append(&w, "before");
    std::thread::sleep(std::time::Duration::from_millis(5));
    let mark = now_iso();
    std::thread::sleep(std::time::Duration::from_millis(5));
    append(&w, "after-1");
    append(&w, "after-2");
    let r = w
        .read(&WorklogReadOptions {
            since: Some(mark),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(contents(&r), vec!["after-2", "after-1"]);
    assert!(r.iter().all(|e| e.id != before.id));
    // 파싱 안 되는 since 는 무시된다.
    let all = w
        .read(&WorklogReadOptions {
            since: Some("garbage".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(all.len(), 3);
}

#[test]
fn read_returns_empty_when_file_missing() {
    let (_d, w) = temp_worklog();
    assert!(w.read(&WorklogReadOptions::default()).unwrap().is_empty());
}

#[test]
fn search_matches_case_insensitively_across_fields() {
    let (_d, w) = temp_worklog();
    w.append(&WorklogAppendInput {
        content: "Decided to use Bun".into(),
        kind: Some("decision".into()),
        ..Default::default()
    })
    .unwrap();
    w.append(&WorklogAppendInput {
        content: "Blocked on auth".into(),
        kind: Some("blocker".into()),
        ..Default::default()
    })
    .unwrap();
    w.append(&WorklogAppendInput {
        content: "User confirmed PRD".into(),
        kind: Some("answer".into()),
        tags: Some(vec!["prd".into()]),
        ..Default::default()
    })
    .unwrap();
    w.append(&WorklogAppendInput {
        content: "linked page".into(),
        page_id: Some(PAGE.into()),
        ..Default::default()
    })
    .unwrap();

    let r = w.search("BUN", &WorklogSearchOptions::default()).unwrap();
    assert_eq!(contents(&r), vec!["Decided to use Bun"]);
    let r = w.search("prd", &WorklogSearchOptions::default()).unwrap();
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].kind, "answer");
    let r = w
        .search(&PAGE_DASHED[0..8], &WorklogSearchOptions::default())
        .unwrap();
    assert_eq!(r[0].page_id.as_deref(), Some(PAGE_DASHED));
    let r = w
        .search(
            "",
            &WorklogSearchOptions {
                kind: Some("blocker".into()),
                limit: None,
            },
        )
        .unwrap();
    assert_eq!(contents(&r), vec!["Blocked on auth"]);
    for i in 0..30 {
        append(&w, &format!("noise {i}"));
    }
    let r = w
        .search(
            "noise",
            &WorklogSearchOptions {
                limit: Some(3),
                kind: None,
            },
        )
        .unwrap();
    assert_eq!(r.len(), 3);
}

#[test]
fn status_before_and_after_writes() {
    let (_d, w) = temp_worklog();
    let s = w.status().unwrap();
    assert!(!s.exists);
    assert_eq!(s.total_entries, 0);
    assert_eq!(s.size_bytes, 0);
    assert!(s.last_entry_at.is_none());
    assert_eq!(s.dir_source, WorklogDirSource::Config);
    assert_eq!(s.project_key, "t-00000000");

    append(&w, "a");
    let last = append(&w, "b");
    let s = w.status().unwrap();
    assert!(s.exists);
    assert_eq!(s.total_entries, 2);
    assert!(s.size_bytes > 0);
    assert_eq!(s.last_entry_at.as_deref(), Some(last.timestamp.as_str()));
    assert!(s.last_digest_at.is_none());

    let mark = w
        .append(&WorklogAppendInput {
            content: "digest of 2".into(),
            kind: Some("digest".into()),
            ..Default::default()
        })
        .unwrap();
    let s = w.status().unwrap();
    assert_eq!(s.last_digest_at.as_deref(), Some(mark.timestamp.as_str()));
    // TS 와 같은 JSON 모양 — camelCase, 없는 필드는 생략.
    let json = serde_json::to_value(&s).unwrap();
    let keys: Vec<&str> = json
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        vec![
            "path",
            "exists",
            "totalEntries",
            "sizeBytes",
            "projectKey",
            "dirSource",
            "lastEntryAt",
            "lastDigestAt"
        ]
    );
    assert_eq!(json["dirSource"], "config");
}

#[test]
fn dir_source_invariant_and_env_precedence() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().to_string_lossy().to_string();
    let clamped = Worklog::new(WorklogOptions {
        base_dir: Some(base.clone()),
        dir_source: Some(WorklogDirSource::Default),
        project_key: Some("k".into()),
        ..Default::default()
    });
    assert_eq!(
        clamped.status().unwrap().dir_source,
        WorklogDirSource::Config
    );
    let env = Worklog::new(WorklogOptions {
        base_dir: Some(base.clone()),
        dir_source: Some(WorklogDirSource::Env),
        project_key: Some("k".into()),
        ..Default::default()
    });
    assert_eq!(env.status().unwrap().dir_source, WorklogDirSource::Env);

    let from_env = Worklog::from_env(
        Some(&base),
        Some("/tmp/config-dir"),
        Some(dir.path().into()),
    );
    assert_eq!(from_env.dir(), Path::new(&base));
    assert_eq!(from_env.status().unwrap().dir_source, WorklogDirSource::Env);
    let from_config = Worklog::from_env(None, Some(&base), Some(dir.path().into()));
    assert_eq!(
        from_config.status().unwrap().dir_source,
        WorklogDirSource::Config
    );
    let default = Worklog::from_env(None, None, Some(dir.path().into()));
    assert_eq!(
        default.status().unwrap().dir_source,
        WorklogDirSource::Default
    );
    assert!(default
        .dir()
        .to_string_lossy()
        .contains(".config/rocky/worklog/"));
}

#[test]
fn tilde_base_dir_resolves_under_home() {
    let w = Worklog::new(WorklogOptions {
        base_dir: Some("~/rocky-j-test".into()),
        project_key: Some("k".into()),
        ..Default::default()
    });
    let home = std::env::var("HOME").unwrap();
    assert_eq!(w.dir(), PathBuf::from(home).join("rocky-j-test"));
}

#[test]
fn project_key_is_stable_sanitized_basename_plus_hash() {
    let no_git = |_: &Path| None;
    let a = default_project_key(Path::new("/Users/x/my project!/app"), &no_git);
    let b = default_project_key(Path::new("/Users/x/my project!/app"), &no_git);
    assert_eq!(a, b);
    assert!(a.starts_with("app-"));
    assert_eq!(a.len(), "app-".len() + 8);
    assert_ne!(
        default_project_key(Path::new("/a/app"), &no_git),
        default_project_key(Path::new("/b/app"), &no_git)
    );
    assert!(default_project_key(Path::new("/tmp/scratch"), &no_git).starts_with("scratch-"));
    assert!(default_project_key(Path::new("/Users/x/!!!"), &no_git).starts_with("project-"));
}

#[test]
fn project_key_matches_ts_golden() {
    // 실제 앵커: ~/.config/rocky/worklog/rocky-4745b950 = sha1("/Users/minjun/dev/workspaces/rocky")[:8].
    // 경로가 이 머신에 없어도(CI) canonicalize 가 절대 경로 그대로 돌려주므로 같은 값이다.
    let key = default_project_key(Path::new("/Users/minjun/dev/workspaces/rocky"), &|_| {
        Some(".git".into())
    });
    assert_eq!(key, "rocky-4745b950");
    let key = default_project_key(
        Path::new("/Users/minjun/dev/workspaces/rocky-todo"),
        &|_| Some(".git".into()),
    );
    assert_eq!(key, "rocky-todo-04340422");
}

#[test]
fn project_key_folds_worktree_onto_main_workspace() {
    let git = |_: &Path| Some("/ws/my-repo/.git".to_string());
    let from_worktree = default_project_key(Path::new("/orca/workspaces/my-repo/eelpout"), &git);
    let from_main = default_project_key(Path::new("/ws/my-repo"), &|_| Some(".git".into()));
    assert_eq!(from_worktree, from_main);
    assert!(from_worktree.starts_with("my-repo-"));
    // separate-git-dir / bare: common dir 자체가 식별자.
    let store = |_: &Path| Some("/elsewhere/store/app.git".to_string());
    let key = default_project_key(Path::new("/ws/app"), &store);
    assert!(key.starts_with("app-git-"));
    assert_eq!(default_project_key(Path::new("/ws/app-wt"), &store), key);
}

#[test]
fn real_git_worktree_folds_onto_repo_root() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("main-repo");
    std::fs::create_dir_all(&repo).unwrap();
    let run = |args: &[&str], cwd: &Path| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .unwrap()
    };
    run(&["init", "-q", "-b", "main"], &repo);
    run(&["config", "user.email", "t@example.com"], &repo);
    run(&["config", "user.name", "T"], &repo);
    std::fs::write(repo.join("f.txt"), "hi\n").unwrap();
    run(&["add", "."], &repo);
    run(&["commit", "-qm", "init"], &repo);
    let wt = root.path().join("eelpout");
    let added = run(
        &["worktree", "add", "-q", "-b", "feat", wt.to_str().unwrap()],
        &repo,
    );
    assert!(added.status.success());
    let git = &git_common_dir;
    assert_eq!(
        resolve_repo_root(&wt, git),
        std::fs::canonicalize(&repo).unwrap()
    );
    assert_eq!(
        default_project_key(&wt, git),
        default_project_key(&repo, git)
    );
}

#[test]
fn graceful_degradation_on_corrupt_lines() {
    let (d, w) = temp_worklog();
    let file = d.path().join(WORKLOG_FILE);
    append(&w, "first");
    std::fs::OpenOptions::new()
        .append(true)
        .open(&file)
        .unwrap()
        .write_all(b"{ this is not json\n")
        .unwrap();
    append(&w, "second");
    let r = w.read(&WorklogReadOptions::default()).unwrap();
    assert_eq!(contents(&r), vec!["second", "first"]);

    std::fs::OpenOptions::new()
        .append(true)
        .open(&file)
        .unwrap()
        .write_all(b"{\"id\":\"x\",\"timestamp\":\"now\"}\n{\"id\":\"p\",\"ti")
        .unwrap();
    let r = w.read(&WorklogReadOptions::default()).unwrap();
    assert_eq!(contents(&r), vec!["second", "first"]);
}

use std::io::Write;

#[test]
fn append_after_unterminated_last_line_keeps_both_lines_separate() {
    let (d, w) = temp_worklog();
    let file = d.path().join(WORKLOG_FILE);
    std::fs::write(&file, "{\"id\":\"crashed\",\"timestamp\":\"").unwrap();
    let fresh = append(&w, "after-restart");
    let r = w.read(&WorklogReadOptions::default()).unwrap();
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].id, fresh.id);
}

#[test]
fn garbage_file_reads_as_empty_but_exists() {
    let (d, w) = temp_worklog();
    std::fs::write(d.path().join(WORKLOG_FILE), "garbage\n}{also garbage\n").unwrap();
    assert!(w.read(&WorklogReadOptions::default()).unwrap().is_empty());
    let s = w.status().unwrap();
    assert!(s.exists);
    assert_eq!(s.total_entries, 0);
}

#[test]
fn non_missing_io_errors_surface() {
    // worklog.jsonl 자리에 디렉터리 — read 가 ENOENT 가 아닌 오류로 실패해야 한다.
    let (d, w) = temp_worklog();
    std::fs::create_dir_all(d.path().join(WORKLOG_FILE)).unwrap();
    assert!(w.read(&WorklogReadOptions::default()).is_err());
    assert!(w.search("x", &WorklogSearchOptions::default()).is_err());
    assert!(w.status().is_err());
}

#[test]
fn page_id_normalization_accepts_dashed_undashed_and_urls() {
    assert_eq!(normalize_page_id(PAGE).unwrap(), PAGE_DASHED);
    assert_eq!(normalize_page_id(PAGE_DASHED).unwrap(), PAGE_DASHED);
    assert_eq!(
        normalize_page_id(&format!("https://www.notion.so/team/Title-{PAGE}?v=1")).unwrap(),
        PAGE_DASHED
    );
    assert_eq!(
        normalize_page_id("1234ABCD1234ABCD1234ABCD1234ABCD").unwrap(),
        PAGE_DASHED
    );
    assert!(normalize_page_id("not-a-page").is_err());
}

#[test]
fn timestamp_parsing_covers_js_shapes() {
    assert!(parse_timestamp_ms("2026-09-22T00:00:00.000Z").is_some());
    assert!(parse_timestamp_ms("2026-09-22T00:00:00Z").is_some());
    assert!(parse_timestamp_ms("2026-09-22T09:00:00+09:00").is_some());
    assert!(parse_timestamp_ms("2026-09-22").is_some());
    assert!(parse_timestamp_ms("now").is_none());
}
