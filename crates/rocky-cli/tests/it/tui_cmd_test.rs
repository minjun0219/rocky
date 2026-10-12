//! `rocky tui` 가 bun 에 넘기는 인자.

use rocky_cli::tui_cmd::tui_args;

#[test]
fn passes_port_and_board_and_never_lets_bun_install() {
    assert_eq!(
        tui_args("/x/tui.js", 8636, "rocky", None),
        [
            "--no-install",
            "/x/tui.js",
            "--port",
            "8636",
            "--board",
            "rocky"
        ]
    );
}

#[test]
fn passes_actor_only_when_given() {
    let args = tui_args("/x/tui.js", 8699, "mdwire", Some("logan"));
    assert_eq!(args[args.len() - 2..], ["--actor", "logan"]);
    // 빈 값은 넘기지 않는다 — TUI 가 자기 기본값(ROCKY_TODO_ACTOR·웹과 같은 이름)을 쓴다
    assert!(!tui_args("/x/tui.js", 8636, "rocky", Some("  ")).contains(&"--actor".to_string()));
}
