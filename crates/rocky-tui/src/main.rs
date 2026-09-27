//! rocky-tui 진입 — 설정에서 포트를 읽고, 보드를 고르고, SSE 를 붙인 뒤 그리기 루프.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event as TermEvent};
use rocky_core::config::{
    env_snapshot, load_todo_config, resolve_runtime_config, user_config_path,
};
use rocky_tui::api::Api;
use rocky_tui::app::{
    key_to_action, pick_board, promote_body, Action, App, Picker, PickerOutcome, Tab,
};
use rocky_tui::events::{self, Event};
use rocky_tui::{github, ui};

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
    // 열린 핸드오프 — 실패해도 목록은 그대로(표시만 빠진다).
    if let Ok(handoffs) = api.open_handoffs(&app.board) {
        app.handoffs = handoffs;
    }
    // 상세는 **항상** 다시 — ref 가 같아도 댓글·히스토리는 다른 클라이언트가 바꿨을 수 있다
    // (목록의 comment_count 만 갱신되고 오른쪽 댓글이 오래된 채 남는 걸 막는다).
    if let Some(todo_ref) = app.selected_todo().map(|t| t.r#ref.clone()) {
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

fn refetch_inbox(app: &mut App, api: &Api, refresh: bool) {
    match api.inbox(refresh) {
        Ok(inbox) => app.set_inbox(inbox),
        Err(error) => app.notice = Some(error),
    }
}

/// 선택 항목의 GitHub 링크를 백그라운드에서 조회한다 — 화면은 막지 않는다.
fn schedule_gh(app: &mut App, tx: &mpsc::Sender<Event>) {
    for url in app.gh_needed() {
        app.gh_pending.insert(url.clone());
        github::spawn_fetch(url, tx.clone());
    }
}

/// `h` — 후보가 정확히 1개면 바로, 아니면 피커.
fn start_handoff(app: &mut App, api: &Api) {
    let Some(todo_ref) = app.selected_todo().map(|t| t.r#ref.clone()) else {
        return;
    };
    let sessions = match api.sessions(&app.board) {
        Ok(s) => s,
        Err(error) => {
            app.notice = Some(error);
            return;
        }
    };
    if !sessions.available {
        app.notice = Some(format!(
            "세션 목록을 볼 수 없다: {}",
            sessions.reason.unwrap_or_else(|| "claude CLI 없음".into())
        ));
        return;
    }
    let matched: Vec<_> = sessions.sessions.iter().filter(|s| s.matched).collect();
    if matched.len() == 1 {
        send_handoff(app, api, &todo_ref, &matched[0].session_id.clone());
        return;
    }
    if sessions.sessions.is_empty() {
        app.notice = Some("실행 중인 세션이 없다 — n 으로 새 세션을 띄운다".into());
        return;
    }
    // 매칭된 세션을 앞에 — 피커의 첫 후보가 가장 그럴듯한 것이 되게.
    let mut choices = sessions.sessions;
    choices.sort_by_key(|s| !s.matched);
    app.picker = Some(Picker {
        todo_ref,
        choices,
        selected: 0,
    });
}

fn send_handoff(app: &mut App, api: &Api, todo_ref: &str, session_id: &str) {
    match api.handoff(todo_ref, session_id) {
        Ok(_) => app.notice = Some(format!("{todo_ref} → 세션에 넘김 — 다음 턴에 집어간다")),
        Err(error) => app.notice = Some(error),
    }
    refetch(app, api);
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
    events::spawn(api.base_url.clone(), tx.clone());
    schedule_gh(&mut app, &tx);

    // ratatui::init 은 tty 가 아니면 패닉한다 — 파이프·CI 에서 부르면 사람이 읽을 한 줄로.
    {
        use std::io::IsTerminal;
        if !std::io::stdout().is_terminal() {
            return Err("rocky-tui 는 터미널에서만 돈다 (stdout 이 tty 가 아니다)".into());
        }
    }
    let mut terminal = ratatui::init();
    let result = run(&mut terminal, &mut app, &api, &tx, &rx);
    ratatui::restore();
    result
}

fn run(
    terminal: &mut ratatui::DefaultTerminal,
    app: &mut App,
    api: &Api,
    tx: &mpsc::Sender<Event>,
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
                // 피커가 열려 있으면 키는 전부 피커 것.
                if let Some(picker) = app.picker.as_mut() {
                    match picker.handle_key(key) {
                        PickerOutcome::None => {}
                        PickerOutcome::Cancel => app.picker = None,
                        PickerOutcome::Confirm(todo_ref, session_id) => {
                            app.picker = None;
                            send_handoff(app, api, &todo_ref, &session_id);
                        }
                    }
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
                    Action::ToggleTab => {
                        app.toggle_tab();
                        if app.tab == Tab::Inbox && app.inbox.is_none() {
                            refetch_inbox(app, api, false);
                        }
                    }
                    Action::Refresh => {
                        refetch_boards(app, api);
                        refetch(app, api);
                        if app.tab == Tab::Inbox {
                            refetch_inbox(app, api, true);
                        }
                        app.gh.clear();
                    }
                    Action::Status(action) => {
                        if let Some(todo_ref) = app.selected_todo().map(|t| t.r#ref.clone()) {
                            match api.status(&todo_ref, action) {
                                Ok(_) => refetch(app, api),
                                Err(error) => app.notice = Some(error),
                            }
                        }
                    }
                    Action::Promote => {
                        if app.tab == Tab::Inbox {
                            if let Some((source, item)) = app.selected_inbox_item() {
                                if app.is_promoted(item) {
                                    app.notice = Some("이미 올라간 항목이다".into());
                                } else {
                                    let body = promote_body(&app.board, &source.name, item);
                                    match api.create_todo(&body) {
                                        Ok(todo) => {
                                            app.notice = Some(format!(
                                                "{} 로 올림: {}",
                                                todo.r#ref, todo.todo.title
                                            ));
                                            refetch(app, api);
                                        }
                                        Err(error) => app.notice = Some(error),
                                    }
                                }
                            }
                        }
                    }
                    Action::Handoff => {
                        if app.tab == Tab::Board {
                            start_handoff(app, api);
                        }
                    }
                    Action::Spawn => {
                        if let Some(todo_ref) = app.selected_todo().map(|t| t.r#ref.clone()) {
                            match api.spawn(&todo_ref) {
                                Ok(out) => {
                                    let reused =
                                        out.get("reused").and_then(|v| v.as_bool()) == Some(true);
                                    let path = out
                                        .get("worktreePath")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("");
                                    app.notice = Some(if reused {
                                        format!("{todo_ref}: 이미 도는 세션에 넘김 ({path})")
                                    } else {
                                        format!("{todo_ref}: 새 세션을 띄웠다 ({path})")
                                    });
                                    refetch(app, api);
                                }
                                Err(error) => app.notice = Some(error),
                            }
                        }
                    }
                    Action::Issue => {
                        if let Some(todo_ref) = app.selected_todo().map(|t| t.r#ref.clone()) {
                            match api.issue(&todo_ref) {
                                Ok(out) => {
                                    let url = out.get("url").and_then(|v| v.as_str()).unwrap_or("");
                                    app.notice = Some(format!("이슈 생성: {url}"));
                                    refetch(app, api);
                                }
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
                schedule_gh(app, tx);
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
                Event::Gh(url, summary) => app.set_gh(url, summary),
            }
        }
        if need_boards {
            refetch_boards(app, api);
        }
        if need_refetch {
            refetch(app, api);
            schedule_gh(app, tx);
        }

        // 데몬이 없으면 주기적으로 다시 두드린다 — SSE 재연결과 별개로 REST 도 회복해야 한다.
        if !app.daemon_ok && last_retry.elapsed() >= RETRY {
            last_retry = Instant::now();
            refetch(app, api);
        }
    }
}
