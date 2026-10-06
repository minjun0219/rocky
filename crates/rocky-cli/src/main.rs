//! `rocky` 진입점.

use rocky_cli::commands::{self, Printer};
use rocky_cli::context::{build_cli_context, infer_board_key};
use rocky_cli::flags::parse_flags;
use rocky_cli::HELP;

fn main() {
    // Rust 런타임은 SIGPIPE 를 무시해 두므로 `rocky daemon status | head -2` 처럼 읽는 쪽이 먼저
    // 닫히면 `println!` 이 EPIPE 로 **패닉**한다(실측: "failed printing to stdout: Broken pipe").
    // 파이프에 쓰는 CLI 는 셸 도구답게 조용히 죽어야 한다 — 기본 동작(종료)으로 되돌린다.
    // SAFETY: signal(2) 에 상수 둘을 넘길 뿐이고, 스레드가 생기기 전 main 첫 줄에서 부른다.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if let Err(error) = run(&argv) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run(argv: &[String]) -> Result<(), String> {
    let parsed = match parse_flags(argv) {
        Ok(parsed) => parsed,
        // statusLine 명령줄이 틀려도 statusline 은 비지 않는다 — 무엇이 틀렸는지 한 줄을 낸다(cc-usage 와 같다).
        Err(e)
            if argv.first().is_some_and(|c| c == "statusline")
                && argv.iter().any(|a| a == "--full") =>
        {
            println!("[rocky] {e}");
            return Ok(());
        }
        Err(e) => return Err(e),
    };
    let command = parsed.positionals.first().map(String::as_str).unwrap_or("");
    let rest: Vec<String> = parsed.positionals.iter().skip(1).cloned().collect();

    // `version` 은 데몬을 건드리지 않는다. 두 형태(`version` / `--version`)를 한 표면
    // `rocky version` 으로 남긴다 — 표면 판단은 사용 로그의 숫자로 한다(KNOWN_SURFACES).
    if command == "version" || parsed.bool_flag("version") {
        println!("rocky {}", env!("CARGO_PKG_VERSION"));
        rocky_cli::usage_cmd::record(
            rocky_core::usage::UsageSource::Cli,
            "rocky version",
            true,
            None,
            None,
        );
        return Ok(());
    }
    // `help` 와 인자 없음은 데몬을 건드리지 않는다 — 도움말 보려다 데몬이 뜨면 곤란하다.
    if command.is_empty() || command == "help" || parsed.bool_flag("help") {
        println!("{HELP}");
        return Ok(());
    }

    let (ctx, runtime, todo_config) = build_cli_context(parsed.str_flag("actor"));
    let expose_lan = runtime
        .expose
        .contains(&rocky_core::config::ExposeChannel::Lan);
    let expose_ts = runtime
        .expose
        .contains(&rocky_core::config::ExposeChannel::TailscaleServe);
    // statusline 은 1초마다 × 세션 수만큼 도는 자리다 — 사용 로그·데몬 자동 기동을 거치지 않는다.
    // statusline 이 detached 로 띄우는 갱신 — 사람이 부를 일은 없다(도움말에도 없다).
    if command == "statusline" && rest.first().map(String::as_str) == Some("refresh") {
        let mut cfg =
            rocky_core::config::load_statusline_block(&rocky_core::config::user_config_path());
        // 부모 statusline 이 정한 source 를 환경 변수로 받는다(`statusline_refresh::spawn_detached`).
        let env = std::env::var(rocky_core::limits::SOURCE_ENV).ok();
        cfg.limits.source =
            rocky_core::limits::override_source(cfg.limits.source, env.as_deref(), None)
                .unwrap_or(cfg.limits.source);
        rocky_cli::statusline_refresh::run(&cfg, commands::statusline_now());
        return Ok(());
    }
    // guard·allow 같은 statusline 의 하위 명령은 사람이 부르거나 prompt 당 한 번 도는 것이라 보통 경로(사용 로그)를 탄다.
    let statusline_tool = matches!(
        rest.first().map(String::as_str),
        Some("guard" | "allow" | "probe" | "doctor")
    );
    if command == "statusline" && !statusline_tool {
        commands::cmd_statusline(
            &ctx,
            parsed.str_flag("cwd"),
            parsed.str_flag("session"),
            parsed.bool_flag("full"),
            parsed.str_flag("source"),
        );
        return Ok(());
    }
    let board = parsed
        .str_flag("board")
        .map(str::to_string)
        .unwrap_or_else(infer_board_key);
    let printer = Printer {
        json: parsed.bool_flag("json"),
    };

    // 사용 로그 — 데몬을 안 거치는 표면은 CLI 가 직접 남긴다. 훅은 이름으로 따로, worklog MCP 는
    // 도구 단위로 자기가 기록하므로 여기서는 뺀다.
    let started = std::time::Instant::now();
    let usage_name = match command {
        "section" | "note" | "board" | "daemon" | "mcp" | "tailscale" | "config" | "inbox"
        | "pr" | "tokens" | "rc" | "statusline" => match rest.first() {
            Some(sub) => format!("rocky {command} {sub}"),
            None => format!("rocky {command}"),
        },
        _ => format!("rocky {command}"),
    };
    let result = match command {
        "ls" => commands::cmd_ls(&ctx, &parsed, &board, &printer),
        "add" => commands::cmd_add(&ctx, &rest, &parsed, &board, &printer),
        "show" => commands::cmd_show(&ctx, &rest, &board, &printer),
        "edit" => commands::cmd_edit(&ctx, &rest, &parsed, &board, &printer),
        "update" if commands::looks_like_todo_edit(&rest, &parsed) => Err(
            "할 일 수정은 `rocky edit REF [플래그]` 로 바뀌었다 — `rocky update` 는 플러그인·데몬을 최신 릴리스로 올린다".into(),
        ),
        "comment" => commands::cmd_comment(&ctx, &rest, &board, &printer),
        "move" => commands::cmd_move(&ctx, &rest, &parsed, &board, &printer),
        "sessions" => commands::cmd_sessions(&ctx, &board, &printer),
        "pr" => commands::cmd_pr(&ctx, &rest, &parsed, &board, &printer),
        "spawn" => commands::cmd_spawn(&ctx, &rest, &parsed, &board, &printer),
        "section" => commands::cmd_section(&ctx, &rest, &board, &printer),
        "handoff" => commands::cmd_handoff(&ctx, &rest, &parsed, &board, &printer),
        "board" => commands::cmd_board(&ctx, &rest, &board, parsed.bool_flag("clear"), &printer),
        "history" => commands::cmd_history(&ctx, &rest, &parsed, &board, &printer),
        "next" => commands::cmd_next(&ctx, &parsed, &board, &printer),
        "today" => commands::cmd_today(&ctx, &printer),
        "inbox" => commands::cmd_inbox(&ctx, &rest, &printer),
        "update" => commands::cmd_update(&ctx, parsed.bool_flag("check")),
        // 0.36.0 에 `upgrade` 로 나갔다 — 한 릴리스 동안 숨은 별칭으로 받는다(도움말에 없다). 0.36.0 에서
        // 올리는 사람은 옛 바이너리의 `upgrade` 를 부르므로, 올린 뒤의 손버릇만 받아 주면 된다.
        "upgrade" => {
            eprintln!("(rocky upgrade 는 rocky update 로 바뀌었다)");
            commands::cmd_update(&ctx, parsed.bool_flag("check"))
        }
        "note" => commands::cmd_note(&ctx, &rest, &parsed, &board, &printer),
        "issue" => commands::cmd_issue(&ctx, &rest, &parsed, &board, &printer),
        "open" => commands::cmd_open(&ctx, expose_lan, expose_ts),
        "daemon" => commands::cmd_daemon(&ctx, &rest, expose_lan, expose_ts),
        "config" => rocky_cli::config_cmd::cmd_config(&ctx, &rest, &todo_config, &printer),
        "usage" => rocky_cli::usage_cmd::cmd_usage(&parsed, &printer),
        "tokens" => rocky_cli::tokens_cmd::cmd_tokens(&ctx, &rest, &parsed, &printer),
        "verify" => rocky_cli::verify_cmd::cmd_verify(&ctx, &rest, &parsed, &printer),
        "rc" => rocky_cli::rc_cmd::cmd_rc(&ctx, &rest, &parsed, &printer),
        "statusline" => {
            let cfg =
                rocky_core::config::load_statusline_block(&rocky_core::config::user_config_path());
            let now = commands::statusline_now();
            match rest.first().map(String::as_str) {
                // 막으면 exit 2 — Claude Code 가 prompt 를 버리고 stderr 를 보인다. 막는 것도 정상 동작이라 성공으로 남긴다.
                Some("guard") => {
                    if let Some(reason) = rocky_cli::statusline_guard::check(&cfg, now) {
                        eprintln!("{}", rocky_cli::statusline_guard::block_message(&reason));
                        rocky_cli::usage_cmd::record(
                            rocky_core::usage::UsageSource::Cli,
                            &usage_name,
                            true,
                            Some(started),
                            None,
                        );
                        std::process::exit(2);
                    }
                    Ok(())
                }
                Some("probe") => rocky_cli::statusline_doctor::probe(&cfg, now)
                    .map(|body| println!("{body}")),
                Some("doctor") => rocky_cli::statusline_doctor::doctor(
                    &cfg,
                    &rocky_core::config::user_config_path(),
                    parsed.str_flag("session"),
                    now,
                ),
                _ => rocky_cli::statusline_guard::allow(rest.get(1).map(String::as_str), now)
                    .map(|message| println!("{message}")),
            }
        }
        "mcp" if rest.first().map(String::as_str) == Some("worklog") => {
            rocky_cli::worklog_mcp::serve_stdio(if parsed.bool_flag("roots") {
                rocky_cli::worklog_mcp::ProjectSource::Roots
            } else {
                rocky_cli::worklog_mcp::ProjectSource::Cwd
            })
        }
        "mcp" => commands::cmd_mcp(&ctx, &rest),
        "tailscale" => commands::cmd_tailscale(&ctx, &rest),
        // 훅 엔트리 — hooks.json 이 부른다. 셋 다 fail-open 이라 항상 Ok.
        "hook" => {
            use rocky_cli::hooks;
            let hook = rest.first().map(String::as_str).unwrap_or("");
            match hook {
                "ensure-daemon" => hooks::hook_ensure_daemon(&ctx, todo_config.session_summary),
                "notify-todo" => hooks::hook_notify_todo(&ctx, todo_config.watch),
                "handoff-stop" => hooks::hook_handoff_stop(&ctx),
                "log-turn" => hooks::hook_log_turn(&ctx),
                "claim-doing" => hooks::hook_claim_doing(&ctx),
                _ => {
                    return Err(
                        "usage: rocky hook ensure-daemon|notify-todo|handoff-stop|log-turn|claim-doing"
                            .into(),
                    )
                }
            }
            rocky_cli::usage_cmd::record(
                rocky_core::usage::UsageSource::Hook,
                &format!("hook {hook}"),
                true,
                Some(started),
                None,
            );
            Ok(())
        }
        "start" | "stop" | "done" | "reopen" | "archive" | "unarchive" => {
            commands::cmd_status(&ctx, command, &rest, &board, &printer)
        }
        // 문구를 한국어로 바꾸지 않는다 — TS 판과 같은 문자열이어야 parity 게이트가
        // 의미를 갖고, 이미 이 메시지를 잡는 스크립트가 있을 수 있다.
        other => Err(format!("unknown command: {other}\n\n{HELP}")),
    };
    let is_worklog_mcp = command == "mcp" && rest.first().map(String::as_str) == Some("worklog");
    // 모르는 명령(오타)은 표면이 아니라 이름 공간만 더럽힌다 — 남기지 않는다.
    let unknown = matches!(&result, Err(e) if e.starts_with("unknown command:"));
    if command != "hook" && !is_worklog_mcp && !unknown {
        rocky_cli::usage_cmd::record(
            rocky_core::usage::UsageSource::Cli,
            &usage_name,
            result.is_ok(),
            Some(started),
            None,
        );
    }
    result
}
