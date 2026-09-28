//! `rocky` 진입점.

use rocky_cli::commands::{self, Printer};
use rocky_cli::context::{build_cli_context, infer_board_key};
use rocky_cli::flags::parse_flags;
use rocky_cli::HELP;

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if let Err(error) = run(&argv) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run(argv: &[String]) -> Result<(), String> {
    let parsed = parse_flags(argv)?;
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
        "section" | "note" | "board" | "daemon" | "mcp" | "tailscale" | "config" => {
            match rest.first() {
                Some(sub) => format!("rocky {command} {sub}"),
                None => format!("rocky {command}"),
            }
        }
        _ => format!("rocky {command}"),
    };
    let result = match command {
        "ls" => commands::cmd_ls(&ctx, &parsed, &board, &printer),
        "add" => commands::cmd_add(&ctx, &rest, &parsed, &board, &printer),
        "show" => commands::cmd_show(&ctx, &rest, &board, &printer),
        "update" => commands::cmd_update(&ctx, &rest, &parsed, &board, &printer),
        "comment" => commands::cmd_comment(&ctx, &rest, &board, &printer),
        "move" => commands::cmd_move(&ctx, &rest, &parsed, &board, &printer),
        "sessions" => commands::cmd_sessions(&ctx, &board, &printer),
        "pr" => commands::cmd_pr(&ctx, &parsed, &board, &printer),
        "spawn" => commands::cmd_spawn(&ctx, &rest, &parsed, &board, &printer),
        "section" => commands::cmd_section(&ctx, &rest, &board, &printer),
        "handoff" => commands::cmd_handoff(&ctx, &rest, &parsed, &board, &printer),
        "board" => commands::cmd_board(&ctx, &rest, &board, &printer),
        "history" => commands::cmd_history(&ctx, &rest, &parsed, &board, &printer),
        "next" => commands::cmd_next(&ctx, &parsed, &board, &printer),
        "today" => commands::cmd_today(&ctx, &printer),
        "note" => commands::cmd_note(&ctx, &rest, &parsed, &board, &printer),
        "issue" => commands::cmd_issue(&ctx, &rest, &parsed, &board, &printer),
        "open" => commands::cmd_open(&ctx, expose_lan, expose_ts),
        "daemon" => commands::cmd_daemon(&ctx, &rest, expose_lan, expose_ts),
        "config" => rocky_cli::config_cmd::cmd_config(&ctx, &rest, &todo_config, &printer),
        "usage" => rocky_cli::usage_cmd::cmd_usage(&parsed, &printer),
        "mcp" if rest.first().map(String::as_str) == Some("worklog") => {
            rocky_cli::worklog_mcp::serve_stdio()
        }
        "mcp" => commands::cmd_mcp(&ctx, &rest),
        "tailscale" => commands::cmd_tailscale(&ctx, &rest),
        "tui" => commands::cmd_tui(&rest, parsed.str_flag("board")),
        // 훅 엔트리 — hooks.json 이 부른다. 셋 다 fail-open 이라 항상 Ok.
        "hook" => {
            use rocky_cli::hooks;
            let hook = rest.first().map(String::as_str).unwrap_or("");
            match hook {
                "ensure-daemon" => hooks::hook_ensure_daemon(&ctx, todo_config.session_summary),
                "notify-todo" => hooks::hook_notify_todo(&ctx, todo_config.watch),
                "handoff-stop" => hooks::hook_handoff_stop(&ctx),
                "log-turn" => hooks::hook_log_turn(),
                _ => {
                    return Err(
                        "usage: rocky hook ensure-daemon|notify-todo|handoff-stop|log-turn".into(),
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
