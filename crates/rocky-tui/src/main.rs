//! rocky-tui 진입 — 설정에서 포트를 읽고, 보드를 고르고, SSE 를 붙인 뒤 그리기 루프.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event as TermEvent};
use rocky_core::config::{
    env_snapshot, load_todo_config, resolve_runtime_config, user_config_path,
};
use rocky_tui::api::Api;
use rocky_tui::app::{key_to_action, pick_board, Action, App};
use rocky_tui::events::{self, Event};
use rocky_tui::ui;

const USAGE: &str =
    "rocky-tui [--board KEY] [--port N]\n  보드를 터미널에 띄워 두고 본다. 보드는 cwd 로 유추한다.";

/// 데몬이 없을 때 REST 재시도 간격.
const RETRY: Duration = Duration::from_secs(3);

fn git(args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!text.is_empty()).then_some(text)
}

struct Args {
    board: Option<String>,
    port: Option<u16>,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        board: None,
        port: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--board" => args.board = it.next(),
            "--port" => {
                args.port = Some(
                    it.next()
                        .and_then(|p| p.parse().ok())
                        .ok_or("--port 는 숫자를 받는다")?,
                )
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            other => return Err(format!("unknown flag: {other}\n\n{USAGE}")),
        }
    }
    Ok(args)
}

/// 보드 데이터 한 번 — 실패는 notice 로, daemon_ok 갱신.
fn refetch(app: &mut App, api: &Api) {
    match api
        .sections(&app.board)
        .and_then(|s| api.todos(&app.board).map(|t| (s, t)))
    {
        Ok((sections, todos)) => {
            app.daemon_ok = true;
            app.notice = None;
            app.set_board_data(sections, todos);
        }
        Err(error) => {
            app.daemon_ok = false;
            app.notice = Some(error);
        }
    }
    if let Some(todo_ref) = app.detail_needed() {
        if let Ok(detail) = api.detail(&todo_ref) {
            app.detail = Some(detail);
        }
    }
}

fn refetch_boards(app: &mut App, api: &Api) {
    if let Ok(boards) = api.boards() {
        app.boards = boards;
    }
}

fn main() -> Result<(), String> {
    let args = parse_args()?;
    let runtime = resolve_runtime_config(&env_snapshot(), &load_todo_config(&user_config_path()));
    let port = args.port.unwrap_or(runtime.port);
    let api = Api::new(format!("http://127.0.0.1:{port}"));

    let boards = api.boards().unwrap_or_default();
    let cwd = std::env::current_dir()
        .ok()
        .map(|p| p.to_string_lossy().to_string());
    let board = pick_board(
        args.board.as_deref(),
        &boards,
        cwd.as_deref(),
        git(&["remote", "get-url", "origin"]).as_deref(),
        git(&["rev-parse", "--show-toplevel"]).as_deref(),
    );
    let mut app = App::new(board);
    app.boards = boards;
    app.daemon_ok = api.health();
    refetch(&mut app, &api);

    let (tx, rx) = mpsc::channel::<Event>();
    events::spawn(api.base_url.clone(), tx);

    // ratatui::init 은 tty 가 아니면 패닉한다 — 파이프·CI 에서 부르면 사람이 읽을 한 줄로.
    {
        use std::io::IsTerminal;
        if !std::io::stdout().is_terminal() {
            return Err("rocky-tui 는 터미널에서만 돈다 (stdout 이 tty 가 아니다)".into());
        }
    }
    let mut terminal = ratatui::init();
    let result = run(&mut terminal, &mut app, &api, &rx);
    ratatui::restore();
    result
}

fn run(
    terminal: &mut ratatui::DefaultTerminal,
    app: &mut App,
    api: &Api,
    rx: &mpsc::Receiver<Event>,
) -> Result<(), String> {
    let mut last_retry = Instant::now();
    loop {
        terminal
            .draw(|frame| ui::render(frame, app))
            .map_err(|e| format!("draw: {e}"))?;

        // 키 입력 — 200ms 안에 없으면 SSE 채널을 본다.
        if event::poll(Duration::from_millis(200)).map_err(|e| format!("poll: {e}"))? {
            if let TermEvent::Key(key) = event::read().map_err(|e| format!("read: {e}"))? {
                if key.kind != event::KeyEventKind::Press {
                    continue;
                }
                match key_to_action(key) {
                    Action::None => {}
                    Action::Quit => return Ok(()),
                    Action::Down => app.move_selection(1),
                    Action::Up => app.move_selection(-1),
                    Action::NextBoard | Action::PrevBoard => {
                        let delta = if matches!(key_to_action(key), Action::NextBoard) {
                            1
                        } else {
                            -1
                        };
                        if app.cycle_board(delta).is_some() {
                            refetch(app, api);
                        }
                    }
                    Action::Refresh => {
                        refetch_boards(app, api);
                        refetch(app, api);
                    }
                    Action::Status(action) => {
                        if let Some(todo_ref) = app.selected_todo().map(|t| t.r#ref.clone()) {
                            match api.status(&todo_ref, action) {
                                Ok(_) => refetch(app, api),
                                Err(error) => app.notice = Some(error),
                            }
                        }
                    }
                }
                if let Some(todo_ref) = app.detail_needed() {
                    match api.detail(&todo_ref) {
                        Ok(detail) => app.detail = Some(detail),
                        Err(error) => app.notice = Some(error),
                    }
                }
            }
        }

        // SSE — 여러 이벤트가 쌓여 있어도 refetch 는 한 번.
        let mut need_refetch = false;
        let mut need_boards = false;
        while let Ok(ev) = rx.try_recv() {
            match ev {
                Event::Connected => {
                    app.connected = true;
                    need_boards = true;
                    need_refetch = true;
                }
                Event::Changed => need_refetch = true,
                Event::Disconnected => app.connected = false,
            }
        }
        if need_boards {
            refetch_boards(app, api);
        }
        if need_refetch {
            refetch(app, api);
        }

        // 데몬이 없으면 주기적으로 다시 두드린다 — SSE 재연결과 별개로 REST 도 회복해야 한다.
        if !app.daemon_ok && last_retry.elapsed() >= RETRY {
            last_retry = Instant::now();
            refetch(app, api);
        }
    }
}
