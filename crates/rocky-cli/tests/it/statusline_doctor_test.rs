//! doctor 가 쓰는 조각 — keychain 후보 고르기와 extra 명령 한 번의 결과(statusline 과 같은 경로).

use rocky_core::statusline::extra::{ExtraCommand, Probe, Vars};

#[test]
fn keychain_candidates_are_claude_code_services_in_order() {
    let dump = r#"keychain: "/Users/x/Library/Keychains/login.keychain-db"
    "svce"<blob>="Claude Code-credentials-abc"
    "svce"<blob>="Slack Safe Storage"
    "svce"<blob>="Claude Code-credentials"
    "svce"<blob>="Claude Code-credentials-abc"
    "acct"<blob>="Claude Code-credentials-zzz"
"#;
    assert_eq!(
        rocky_cli::statusline_doctor::keychain_services_from(dump),
        ["Claude Code-credentials-abc", "Claude Code-credentials"]
    );
}

fn run(command: &[&str], timeout_ms: Option<u64>, session: &str) -> (Option<Vec<String>>, Probe) {
    let command = ExtraCommand {
        command: command.iter().map(|s| s.to_string()).collect(),
        timeout_ms,
    };
    rocky_cli::bounded::run_extra(
        &command,
        Vars {
            session_id: session,
            cwd: "/w",
        },
    )
}

#[test]
fn run_extra_tells_why_a_line_did_not_attach() {
    let (argv, probe) = run(&["sh", "-c", "printf 'a\\n\\nb\\n'"], None, "");
    assert_eq!(argv.unwrap()[0], "sh");
    assert!(
        matches!(&probe, Probe::Ok { lines, .. } if lines == &[b"a".to_vec(), b"b".to_vec()]),
        "{probe:?}"
    );

    assert_eq!(
        run(&["t", "{{session_id}}"], None, "").1,
        Probe::Skipped {
            missing: Some("{{session_id}}")
        }
    );
    assert_eq!(run(&[], None, "s").1, Probe::Skipped { missing: None });
    assert!(matches!(
        run(&["rocky-test-no-such-tool"], None, "").1,
        Probe::NotFound(_)
    ));
    assert_eq!(
        run(
            &["sh", "-c", "echo out; echo 'first err\nsecond' >&2; exit 4"],
            None,
            ""
        )
        .1,
        Probe::Failed {
            error: "exit status 4".into(),
            stderr: "first err".into()
        }
    );
    assert_eq!(run(&["sleep", "5"], Some(50), "").1, Probe::Timeout);
    assert!(matches!(run(&["true"], None, "").1, Probe::Empty { .. }));
    // 백그라운드 자식이 stdout 을 붙잡으면 그 출력은 버린다.
    let (_, held) = run(&["sh", "-c", "echo x; sleep 3 &"], Some(2000), "");
    assert!(
        matches!(&held, Probe::Failed { error, .. } if error.contains("붙잡고")),
        "{held:?}"
    );
}
