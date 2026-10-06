//! spawn 의 rc 갈래 — rc 가 켜진 기기에서 할 일의 워크트리에 단일 세션 rc 서버를 띄우고, 그 세션에 핸드오프를 넣어 깨운다.
//! 가짜 프로세스 세계(`ps` · `lsof` · `git` · 띄우기 · 시계)와 진짜 유닉스 소켓(받은편지함)으로 순서를 고정한다.

use std::collections::HashSet;
use std::io::Read;
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use chrono::NaiveDateTime;
use rocky_core::config::RcConfig;
use rocky_core::peer_inbox::InboxRegistration;
use rocky_core::store::TodoStore;
use rocky_core::types::*;
use rockyd::rc::{RcController, RcOps};
use rockyd::runner::{CmdOutput, Runner};
use rockyd::server::ServerState;
use rockyd::sessions_exec::{fixed_sessions, SessionsProvider};
use rockyd::spawnctl::SpawnFn;

use crate::common::*;

const SERVER: u32 = 500;
const CHILD: u32 = 501;
const NAME: &str = "rocky-todo-1: 세션 띄우기";

struct World {
    alive: HashSet<u32>,
    /// 띄운 것 — (argv, 폴더).
    spawned: Vec<(Vec<String>, String)>,
    /// `git` 뒤의 인자 전부(`-C <폴더>` 포함).
    git: Vec<String>,
    clock: NaiveDateTime,
    worktree: String,
    origin_head: Option<&'static str>,
    remote_branches: Vec<&'static str>,
    kept_branch: bool,
    /// 있는 폴더가 git 워크트리인가.
    toplevel_ok: bool,
    /// `worktree add` 가 폴더를 반쯤 만들고 실패한다.
    add_fails: bool,
    /// 띄운 서버가 바로 내려간다 / `already served` / 등록 판정은 됐는데 곧 내려간다 / 세션 자식을 안 만든다.
    dies: bool,
    served: bool,
    vanishes: bool,
    no_session: bool,
    /// 이미 그 워크트리에서 도는 서버(pid 400).
    server_before: bool,
    logged_out: bool,
    ps_fails: bool,
    /// `worktree add` · 띄우기 때 그 할 일에 대기 중 핸드오프가 생긴다(다른 길로 넘겨졌다).
    pending_on_add: Option<(Arc<TodoStore>, String)>,
    pending_on_spawn: Option<(Arc<TodoStore>, String)>,
    /// 서버는 살아 있는데 `ps` 스냅숏에 아직 없다(낡은 캐시).
    hidden: bool,
    /// 서버 pid 의 cwd 가 기록과 다르다(pid 가 재사용됐다).
    moved: bool,
}

fn ok(stdout: String) -> CmdOutput {
    CmdOutput {
        code: 0,
        stdout,
        stderr: String::new(),
    }
}

fn pend(store: &TodoStore, todo_id: &str) {
    store
        .create_handoff(&CreateHandoffInput {
            todo_ref: todo_id.to_string(),
            session_id: "someone-else".into(),
            session_name: None,
            session_cwd: None,
            note: None,
            actor: "logan".into(),
            current_board_id: None,
        })
        .unwrap();
}

fn git_answer(w: &mut World, argv: &[String]) -> CmdOutput {
    w.git.push(argv[1..].join(" "));
    let path = argv
        .iter()
        .find(|a| a.contains(".claude/worktrees"))
        .cloned()
        .unwrap_or_default();
    let sub = argv[3..].join(" ");
    if sub == "rev-parse --show-toplevel" {
        return if w.toplevel_ok {
            ok(format!("{}\n", argv[2]))
        } else {
            CmdOutput::failure("fatal: not a git repository")
        };
    }
    if sub.starts_with("symbolic-ref") {
        return match w.origin_head {
            Some(head) => ok(format!("{head}\n")),
            None => CmdOutput::failure(""),
        };
    }
    if let Some(r) = sub.strip_prefix("rev-parse --verify --quiet refs/remotes/") {
        return if w.remote_branches.contains(&r) {
            ok("abc\n".into())
        } else {
            CmdOutput::failure("")
        };
    }
    if sub.starts_with("rev-parse --verify --quiet refs/heads/") {
        return if w.kept_branch {
            ok("abc\n".into())
        } else {
            CmdOutput::failure("")
        };
    }
    if sub.starts_with("fetch") {
        return ok(String::new());
    }
    if sub.starts_with("worktree add") {
        std::fs::create_dir_all(&path).unwrap();
        if let Some((store, id)) = w.pending_on_add.take() {
            pend(&store, &id);
        }
        return if w.add_fails {
            CmdOutput::failure("fatal: 시간 초과로 끊겼다")
        } else {
            ok(String::new())
        };
    }
    if sub.starts_with("worktree remove --force") {
        std::fs::remove_dir_all(&path).unwrap();
        return ok(String::new());
    }
    CmdOutput::failure("?")
}

fn runner(w: Arc<Mutex<World>>) -> Runner {
    Arc::new(move |argv: Vec<String>, _stdin, _timeout| {
        let mut w = w.lock().unwrap();
        let out = match argv[0].as_str() {
            "ps" if w.ps_fails => CmdOutput::failure("ps: 잠깐 실패"),
            "ps" => {
                let mut ps = String::from("    1     0 30-00:00:00 /sbin/launchd\n");
                if w.server_before {
                    ps.push_str("  400     1    10:00 claude rc --spawn session --name=옛 서버\n");
                }
                if w.alive.contains(&SERVER) && !w.hidden {
                    ps.push_str(&format!(
                        "  {SERVER}     1    00:10 claude rc --spawn session --name={NAME}\n"
                    ));
                    if !w.no_session {
                        ps.push_str(&format!(
                            "  {CHILD}   {SERVER}    00:05 /x/claude --print --sdk-url https://a/v1/code/sessions/cse_1\n"
                        ));
                    }
                }
                ok(ps)
            }
            "lsof" => {
                let mut out = String::new();
                if w.server_before {
                    out.push_str(&format!("p400\nfcwd\nn{}\n", w.worktree));
                }
                if w.alive.contains(&SERVER) && !w.hidden {
                    let cwd = if w.moved {
                        "/w/elsewhere"
                    } else {
                        w.worktree.as_str()
                    };
                    out.push_str(&format!("p{SERVER}\nfcwd\nn{cwd}\n"));
                }
                ok(out)
            }
            "claude" => ok(format!(r#"{{"loggedIn": {}}}"#, !w.logged_out)),
            "git" => git_answer(&mut w, &argv),
            _ => CmdOutput::failure("없음"),
        };
        // 진짜 명령처럼 한 번 양보한다 — 겹친 요청이 예약과 서버 기동 사이에 끼어들 틈이 생긴다.
        Box::pin(async move {
            tokio::task::yield_now().await;
            out
        })
    })
}

fn ops(w: Arc<Mutex<World>>) -> RcOps {
    let (w1, w2, w3, w4) = (w.clone(), w.clone(), w.clone(), w);
    RcOps {
        spawn: Arc::new(move |argv: &[String], dir: &Path, out: &Path, err: &Path| {
            let mut w = w1.lock().unwrap();
            w.spawned
                .push((argv.to_vec(), dir.to_string_lossy().into_owned()));
            if let Some((store, id)) = w.pending_on_spawn.take() {
                pend(&store, &id);
            }
            if w.dies {
                std::fs::write(out, "")?;
                std::fs::write(err, "Error: 무언가 잘못됐다")?;
            } else if w.served {
                std::fs::write(out, "")?;
                std::fs::write(
                        err,
                        "Error: This folder is already served by a terminal `claude remote-control` on this device.",
                    )?;
                w.alive.insert(SERVER);
            } else {
                std::fs::write(out, "·✔︎· Connected · x · main")?;
                std::fs::write(err, "")?;
                if !w.vanishes {
                    w.alive.insert(SERVER);
                }
            }
            Ok(SERVER)
        }),
        signal: Arc::new(move |pid, sig| {
            let mut w = w2.lock().unwrap();
            if sig == 0 {
                return w.alive.contains(&pid);
            }
            w.alive.remove(&pid)
        }),
        // 자지 않고 시계만 돌린다 — 60초 대기도 순식간에.
        sleep: Arc::new(move |d| {
            let mut w = w3.lock().unwrap();
            w.clock += chrono::Duration::from_std(d).unwrap();
            Box::pin(async {})
        }),
        now: Arc::new(move || w4.lock().unwrap().clock),
        binary_mtime: Arc::new(|_| None),
    }
}

struct Rc {
    _dir: tempfile::TempDir,
    /// `<todo dir>/rc` — 핸드오프 기록은 `handoff/<라벨>.json`.
    rc_dir: std::path::PathBuf,
    world: Arc<Mutex<World>>,
    state: Arc<ServerState>,
    worktree: String,
    repo: String,
    todo: Todo,
    listener: UnixListener,
    bg_spawns: Arc<AtomicUsize>,
}

/// rc 가 켜진 기기 — 보드 경로는 진짜 임시 폴더(워크트리가 있는지 디스크로 본다), 받은편지함은 진짜 소켓.
fn rc_fixture(
    f: &Fx,
    sessions: impl FnOnce(&str) -> SessionsProvider,
    setup: impl FnOnce(&mut World, &Todo),
) -> Rc {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo").to_string_lossy().into_owned();
    std::fs::create_dir_all(&repo).unwrap();
    f.store.ensure_board("rocky-todo", None, "logan").unwrap();
    f.store
        .set_board_path("rocky-todo", &repo, "logan")
        .unwrap();
    let todo = f
        .store
        .create_todo(
            &CreateTodoInput {
                board: "rocky-todo".into(),
                title: "세션 띄우기".into(),
                ..Default::default()
            },
            "logan",
        )
        .unwrap();
    let worktree = format!("{repo}/.claude/worktrees/todo-{}", todo.number);
    let sessions = sessions(&worktree);
    let mut world = World {
        alive: HashSet::new(),
        spawned: Vec::new(),
        git: Vec::new(),
        clock: chrono::NaiveDate::from_ymd_opt(2026, 10, 6)
            .unwrap()
            .and_hms_opt(14, 0, 0)
            .unwrap(),
        worktree: worktree.clone(),
        origin_head: Some("origin/main"),
        remote_branches: vec![],
        kept_branch: false,
        toplevel_ok: true,
        add_fails: false,
        dies: false,
        served: false,
        vanishes: false,
        no_session: false,
        server_before: false,
        logged_out: false,
        ps_fails: false,
        pending_on_add: None,
        pending_on_spawn: None,
        hidden: false,
        moved: false,
    };
    setup(&mut world, &todo);
    let world = Arc::new(Mutex::new(world));
    let config = RcConfig {
        root: Some("/w".into()),
        pinned: vec![],
        targets: vec!["repo-a".into()],
        supervise: false,
        nightly: None,
    };
    let home = dir.path().join("home").to_string_lossy().into_owned();
    let control = Arc::new(RcController::new(
        Some(config.clone()),
        home.clone(),
        dir.path().join("rc"),
        runner(world.clone()),
        ops(world.clone()),
    ));
    // 현황 — 캐시 없이 같은 가짜 세계를 잰다(라우트가 핸드오프 서버를 가려 싣는다).
    let status_runner = runner(world.clone());
    let provider: rockyd::rc::RcProvider = Arc::new(move || {
        let (runner, config, home) = (status_runner.clone(), config.clone(), home.clone());
        Box::pin(async move { rockyd::rc::probe(&runner, Some(&config), &home).await })
    });
    let bg_spawns = Arc::new(AtomicUsize::new(0));
    let counter = bg_spawns.clone();
    let bg: SpawnFn = Arc::new(move |_input| {
        counter.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok("5acaaaeb".to_string()) })
    });
    let state = rebuild(f, move |o| {
        o.sessions = Some(sessions);
        o.spawn = Some(bg);
        o.path_exists = Some(Arc::new(|_| true));
        o.real_path = Some(Arc::new(|p| Ok(p.to_string())));
        o.rc_control = Some(control);
        o.rc = Some(provider);
    });
    let socks = dir.path().join("cc-socks-test");
    std::fs::create_dir_all(&socks).unwrap();
    let socket = socks.join(format!("{CHILD}.sock"));
    let listener = UnixListener::bind(&socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    // 서버가 만든 세션이 SessionStart 에서 등록한 것 — 소켓 이름이 그 세션 pid 다. 서버를 띄운 뒤의 등록이어야 고른다.
    state.register_inbox(InboxRegistration {
        session_id: "rc-session-uuid".into(),
        socket: socket.to_string_lossy().into_owned(),
        cwd: worktree.clone(),
        seen_at: chrono::Utc::now().timestamp() + 3600,
        restored: false,
    });
    Rc {
        rc_dir: dir.path().join("rc"),
        _dir: dir,
        world,
        state,
        worktree,
        repo,
        todo,
        listener,
        bg_spawns,
    }
}

fn none(_worktree: &str) -> SessionsProvider {
    fixed_sessions(available(vec![]))
}

fn plain(_: &mut World, _: &Todo) {}

fn drain(listener: &UnixListener) -> Vec<String> {
    let mut out = Vec::new();
    while let Ok((mut stream, _)) = listener.accept() {
        stream.set_nonblocking(false).unwrap();
        let mut text = String::new();
        stream.read_to_string(&mut text).unwrap();
        out.push(text);
    }
    out
}

async fn spawn(rc: &Rc) -> (u16, serde_json::Value) {
    call(
        &rc.state,
        "POST",
        &format!("/api/todos/{}/spawn", rc.todo.id),
        None,
        ReqOptions::default(),
    )
    .await
}

fn error(body: &serde_json::Value) -> &str {
    body["error"].as_str().unwrap_or("")
}

fn handoffs(f: &Fx) -> Vec<Handoff> {
    f.store
        .list_handoffs(&ListHandoffsFilter::default())
        .unwrap()
}

#[tokio::test]
async fn rc_spawn_makes_the_worktree_starts_a_session_server_and_hands_off() {
    let f = fx();
    let rc = rc_fixture(&f, none, plain);
    let (status, body) = spawn(&rc).await;
    assert_eq!(status, 201, "{body}");
    assert_eq!(body["reused"], false);
    assert_eq!(body["worktreePath"], rc.worktree);
    assert_eq!(body["server"]["pid"], SERVER);
    assert_eq!(body["server"]["name"], NAME);
    assert_eq!(body["base"], "origin/main");
    assert_eq!(body["woke"], true);
    assert!(body.get("sessionShortId").is_none());
    assert!(body.get("warning").is_none());
    assert_eq!(body["handoff"]["sessionId"], "rc-session-uuid");
    assert_eq!(body["handoff"]["sessionName"], NAME);
    assert_eq!(
        body["handoff"]["status"], "pending",
        "세션이 턴을 열고 훅이 집는다"
    );

    let w = rc.world.lock().unwrap();
    let repo = &rc.repo;
    assert_eq!(
        w.git,
        vec![
            format!("-C {repo} symbolic-ref --quiet --short refs/remotes/origin/HEAD"),
            format!("-C {repo} fetch --quiet origin main"),
            format!("-C {repo} rev-parse --verify --quiet refs/heads/worktree-todo-1"),
            format!(
                "-C {repo} worktree add -b worktree-todo-1 {} origin/main",
                rc.worktree
            ),
        ],
        "origin 기본 브랜치에서 Claude Code --worktree 와 같은 이름으로 딴다"
    );
    assert_eq!(
        w.spawned,
        vec![(
            vec![
                "claude".to_string(),
                "rc".into(),
                "--spawn".into(),
                "session".into(),
                format!("--name={NAME}"),
            ],
            rc.worktree.clone()
        )]
    );
    drop(w);
    assert_eq!(
        rc.bg_spawns.load(Ordering::SeqCst),
        0,
        "claude --bg 는 안 쓴다"
    );
    let lines = drain(&rc.listener);
    assert_eq!(lines.len(), 1, "한 번 깨운다: {lines:?}");
    assert!(
        lines[0].contains(&format!("rocky-todo-{}", rc.todo.number)),
        "{}",
        lines[0]
    );
}

#[tokio::test]
async fn an_existing_worktree_is_checked_and_used_without_making_one() {
    let f = fx();
    let rc = rc_fixture(&f, none, plain);
    std::fs::create_dir_all(&rc.worktree).unwrap();
    let (status, body) = spawn(&rc).await;
    assert_eq!(status, 201, "{body}");
    assert_eq!(
        rc.world.lock().unwrap().git,
        vec![format!("-C {} rev-parse --show-toplevel", rc.worktree)],
        "그 폴더가 정말 워크트리인지만 본다"
    );
    assert!(body["base"].is_null());
}

#[tokio::test]
async fn a_plain_folder_in_the_worktree_spot_is_refused() {
    let f = fx();
    let rc = rc_fixture(&f, none, |w, _| w.toplevel_ok = false);
    std::fs::create_dir_all(&rc.worktree).unwrap();
    let (status, body) = spawn(&rc).await;
    assert_eq!(status, 400, "{body}");
    assert!(error(&body).contains("git 워크트리가 아니다"), "{body}");
    assert!(
        rc.world.lock().unwrap().spawned.is_empty(),
        "일반 폴더에서 띄우면 세션의 git 이 메인 레포를 잡는다"
    );
}

#[tokio::test]
async fn without_origin_head_it_guesses_main_then_master() {
    let f = fx();
    let rc = rc_fixture(&f, none, |w, _| {
        w.origin_head = None;
        w.remote_branches = vec!["origin/master"];
    });
    let (status, body) = spawn(&rc).await;
    assert_eq!(status, 201, "{body}");
    assert_eq!(body["base"], "origin/master");
    let git = rc.world.lock().unwrap().git.clone();
    assert!(
        git.iter()
            .any(|g| g.ends_with(&format!("{} origin/master", rc.worktree))),
        "{git:?}"
    );
}

#[tokio::test]
async fn a_kept_branch_is_checked_out_again() {
    let f = fx();
    let rc = rc_fixture(&f, none, |w, _| w.kept_branch = true);
    let (status, body) = spawn(&rc).await;
    assert_eq!(status, 201, "{body}");
    let git = rc.world.lock().unwrap().git.clone();
    assert_eq!(
        git.last().unwrap(),
        &format!(
            "-C {} worktree add {} worktree-todo-1",
            rc.repo, rc.worktree
        )
    );
}

#[tokio::test]
async fn a_failed_worktree_add_is_cleaned_up_and_frees_the_reservation() {
    let f = fx();
    let rc = rc_fixture(&f, none, |w, _| w.add_fails = true);
    let (status, body) = spawn(&rc).await;
    assert_eq!(status, 400, "{body}");
    assert!(error(&body).contains("시간 초과로 끊겼다"), "{body}");
    assert!(
        error(&body).contains("반쯤 만든 워크트리는 걷었다"),
        "{body}"
    );
    assert!(
        !Path::new(&rc.worktree).exists(),
        "반쯤 만든 워크트리를 걷는다"
    );
    // 아무것도 안 띄웠으니 곧바로 다시 누를 수 있다.
    rc.world.lock().unwrap().add_fails = false;
    let (again, body) = spawn(&rc).await;
    assert_eq!(again, 201, "{body}");
}

#[tokio::test]
async fn a_server_already_in_that_worktree_is_one_per_todo() {
    let f = fx();
    let rc = rc_fixture(&f, none, |w, _| w.server_before = true);
    let (status, body) = spawn(&rc).await;
    assert_eq!(status, 409, "{body}");
    assert!(error(&body).contains("pid 400"), "{body}");
    assert!(rc.world.lock().unwrap().spawned.is_empty());
    assert!(handoffs(&f).is_empty());
}

#[tokio::test]
async fn a_logged_out_daemon_or_unreadable_status_launches_nothing() {
    let f = fx();
    let rc = rc_fixture(&f, none, |w, _| w.logged_out = true);
    let (status, body) = spawn(&rc).await;
    assert_eq!(status, 409, "{body}");
    assert!(error(&body).contains("로그아웃"), "{body}");

    let f = fx();
    let rc = rc_fixture(&f, none, |w, _| w.ps_fails = true);
    let (status, body) = spawn(&rc).await;
    assert_eq!(status, 409, "{body}");
    assert!(error(&body).contains("rc 현황을 못 읽어"), "{body}");
    assert!(rc.world.lock().unwrap().spawned.is_empty());
}

#[tokio::test]
async fn a_server_without_a_session_is_left_up_named_and_reserved() {
    let f = fx();
    let rc = rc_fixture(&f, none, |w, _| w.no_session = true);
    let (status, body) = spawn(&rc).await;
    assert_eq!(status, 400, "{body}");
    assert!(error(&body).contains(NAME), "{body}");
    assert!(error(&body).contains("그대로 두었다"), "{body}");
    assert!(
        rc.world.lock().unwrap().alive.contains(&SERVER),
        "서버는 내리지 않는다 — 폰 · 웹에서 열 수 있다"
    );
    assert!(handoffs(&f).is_empty(), "넘긴 것이 없으면 기록도 없다");
    // 서버는 떴으니 예약을 남긴다 — 서버 확인과 별개로 예약이 막는다(서버를 지워도 409).
    rc.world.lock().unwrap().alive.clear();
    let (again, body) = spawn(&rc).await;
    assert_eq!(again, 409, "{body}");
    assert!(error(&body).contains("방금 이 워크트리에"), "{body}");
}

#[tokio::test]
async fn a_server_that_dies_at_start_or_while_waiting_frees_the_reservation() {
    let f = fx();
    let rc = rc_fixture(&f, none, |w, _| w.dies = true);
    let (status, body) = spawn(&rc).await;
    assert_eq!(status, 400, "{body}");
    assert!(
        error(&body).contains(&format!(
            "\"{NAME}\"(pid {SERVER}) 가 뜨자마자 내려갔다 — Error: 무언가 잘못됐다"
        )),
        "{body}"
    );
    let (again, body) = spawn(&rc).await;
    assert_eq!(again, 400, "확실히 안 떴으면 예약을 돌려준다: {body}");

    let f = fx();
    let rc = rc_fixture(&f, none, |w, _| w.vanishes = true);
    let (status, body) = spawn(&rc).await;
    assert_eq!(status, 400, "{body}");
    assert!(error(&body).contains("세션 등록 전에 내려갔다"), "{body}");
    assert!(!error(&body).contains("그대로 두었다"), "{body}");
    let (again, _) = spawn(&rc).await;
    assert_eq!(again, 400, "내려간 서버는 예약을 붙들지 않는다");
}

#[tokio::test]
async fn already_served_stops_its_own_server_and_says_when_to_retry() {
    let f = fx();
    let rc = rc_fixture(&f, none, |w, _| w.served = true);
    let (status, body) = spawn(&rc).await;
    assert_eq!(status, 400, "{body}");
    assert!(error(&body).contains("3분쯤 뒤 다시"), "{body}");
    assert!(error(&body).contains("내렸다"), "{body}");
    assert!(!rc.world.lock().unwrap().alive.contains(&SERVER));
}

#[tokio::test]
async fn a_handoff_that_appears_meanwhile_stops_the_launch_or_names_the_server() {
    // 워크트리를 만드는 사이 다른 길로 넘겨졌다 — 서버를 띄우지 않는다.
    let f = fx();
    let store = f.store.clone();
    let rc = rc_fixture(&f, none, |w, todo| {
        w.pending_on_add = Some((store, todo.id.clone()))
    });
    let (status, body) = spawn(&rc).await;
    assert_eq!(status, 409, "{body}");
    assert!(rc.world.lock().unwrap().spawned.is_empty());

    // 띄운 뒤에 생겼다 — 서버는 남고 그 이름을 알린다.
    let f = fx();
    let store = f.store.clone();
    let rc = rc_fixture(&f, none, |w, todo| {
        w.pending_on_spawn = Some((store, todo.id.clone()))
    });
    let (status, body) = spawn(&rc).await;
    assert_eq!(status, 409, "{body}");
    assert!(error(&body).contains(NAME), "{body}");
    assert!(error(&body).contains("그대로 두었다"), "{body}");
}

#[tokio::test]
async fn overlapping_requests_launch_one_server() {
    let f = fx();
    // 세션 목록이 실제로 기다린다 — 두 요청이 예약 확인과 잡기 사이에 겹친다.
    let slow = |_wt: &str| -> SessionsProvider {
        Arc::new(|| {
            Box::pin(async {
                tokio::time::sleep(std::time::Duration::from_millis(30)).await;
                available(vec![])
            })
        })
    };
    let rc = rc_fixture(&f, slow, plain);
    let ((a, _), (b, _)) = tokio::join!(spawn(&rc), spawn(&rc));
    let mut statuses = [a, b];
    statuses.sort();
    assert_eq!(statuses, [201, 409]);
    assert_eq!(rc.world.lock().unwrap().spawned.len(), 1);
}

#[tokio::test]
async fn a_live_session_in_the_worktree_is_reused_and_woken() {
    let f = fx();
    // 세션 목록에 그 워크트리의 세션이 있다 — 받은편지함을 등록한 그 세션이다.
    let live = |wt: &str| {
        fixed_sessions(available(vec![sess(
            i64::from(CHILD),
            wt,
            "rc-session-uuid",
            NAME,
            "idle",
        )]))
    };
    let rc = rc_fixture(&f, live, plain);
    let (status, body) = spawn(&rc).await;
    assert_eq!(status, 201, "{body}");
    assert_eq!(body["reused"], true);
    assert_eq!(body["woke"], true, "쉬는 세션은 깨워야 집는다");
    assert_eq!(body["handoff"]["sessionId"], "rc-session-uuid");
    assert_eq!(drain(&rc.listener).len(), 1);
    assert!(rc.world.lock().unwrap().spawned.is_empty());
}

#[tokio::test]
async fn rc_off_falls_back_to_bg_with_a_warning() {
    let f = fx();
    let rc = rc_fixture(&f, none, plain);
    let state = rebuild(&f, |o| {
        o.sessions = Some(fixed_sessions(available(vec![])));
        o.spawn = Some(Arc::new(|_input| {
            Box::pin(async { Ok("5acaaaeb".to_string()) })
        }));
        o.path_exists = Some(Arc::new(|_| true));
        o.real_path = Some(Arc::new(|p| Ok(p.to_string())));
    });
    let (status, body) = call(
        &state,
        "POST",
        &format!("/api/todos/{}/spawn", rc.todo.id),
        None,
        ReqOptions::default(),
    )
    .await;
    assert_eq!(status, 201, "{body}");
    assert_eq!(body["sessionShortId"], "5acaaaeb");
    assert!(
        body["warning"].as_str().unwrap().contains("ssh"),
        "PR 로 끝나는 일을 못 끝낼 수 있다고 알린다: {body}"
    );
    assert!(body.get("server").is_none());
}

async fn status(rc: &Rc) -> serde_json::Value {
    let (code, body) = call(
        &rc.state,
        "GET",
        "/api/rc/servers",
        None,
        ReqOptions::default(),
    )
    .await;
    assert_eq!(code, 200, "{body}");
    body
}

async fn close(rc: &Rc, key: &str, opts: ReqOptions<'_>) -> (u16, serde_json::Value) {
    call(
        &rc.state,
        "POST",
        &format!("/api/rc/handoffs/{key}/stop"),
        None,
        opts,
    )
    .await
}

#[tokio::test]
async fn a_launched_server_shows_as_a_handoff_and_closes_by_pid() {
    let f = fx();
    let rc = rc_fixture(&f, none, plain);
    let (code, body) = spawn(&rc).await;
    assert_eq!(code, 201, "{body}");
    let s = status(&rc).await;
    assert_eq!(s["handoffs"][0]["name"], NAME, "{s}");
    assert_eq!(s["handoffs"][0]["todoRef"], "rocky-todo-1");
    assert_eq!(s["handoffs"][0]["sessions"], 1);
    assert!(
        s.get("strays")
            .and_then(|v| v.as_array())
            .is_none_or(|v| v.is_empty()),
        "대상 밖에서 뺀다: {s}"
    );

    let (code, body) = close(&rc, "rocky-todo-1", ReqOptions::default()).await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body["down"], true);
    assert_eq!(body["dir"], rc.worktree);
    assert!(
        !rc.world.lock().unwrap().alive.contains(&SERVER),
        "pid 로 내렸다"
    );
    assert!(
        Path::new(&rc.worktree).exists(),
        "워크트리는 남긴다 — 커밋 안 된 작업이 있을 수 있다"
    );
    assert!(!record(&rc).exists(), "닫았으면 기록도 지운다");
    let s = status(&rc).await;
    assert!(s.get("handoffs").is_none(), "{s}");
    let (code, body) = close(&rc, "rocky-todo-1", ReqOptions::default()).await;
    assert_eq!(code, 404);
    assert!(error(&body).contains("핸드오프 서버가 없다"), "{body}");
}

#[tokio::test]
async fn closing_is_local_only_and_never_acts_on_a_failed_probe() {
    let f = fx();
    let rc = rc_fixture(&f, none, plain);
    assert_eq!(spawn(&rc).await.0, 201);
    let remote = ReqOptions {
        peer: Some("100.64.0.1"),
        ..Default::default()
    };
    assert_eq!(close(&rc, "rocky-todo-1", remote).await.0, 403);
    assert_eq!(close(&rc, "rocky-9", ReqOptions::default()).await.0, 404);
    rc.world.lock().unwrap().ps_fails = true;
    let (code, body) = close(&rc, "rocky-todo-1", ReqOptions::default()).await;
    assert_eq!(code, 409, "{body}");
    assert!(
        rc.world.lock().unwrap().alive.contains(&SERVER),
        "현황을 못 읽으면 손대지 않는다"
    );
}

#[tokio::test]
async fn a_server_that_went_away_drops_its_record() {
    let f = fx();
    let rc = rc_fixture(&f, none, plain);
    assert_eq!(spawn(&rc).await.0, 201);
    rc.world.lock().unwrap().alive.clear();
    let (code, body) = close(&rc, "rocky-todo-1", ReqOptions::default()).await;
    assert_eq!(code, 404, "{body}");
    assert!(error(&body).contains("이미 내려가 있다"), "{body}");
    assert!(!record(&rc).exists());
}

fn record(rc: &Rc) -> std::path::PathBuf {
    rc.rc_dir.join("handoff/handoff-rocky-todo-1.json")
}

#[tokio::test]
async fn the_status_keeps_a_record_unless_the_server_is_really_gone() {
    let f = fx();
    let rc = rc_fixture(&f, none, plain);
    assert_eq!(spawn(&rc).await.0, 201);
    assert!(record(&rc).exists());

    // 낡은 스냅숏 — 서버는 살아 있는데 현황에 아직 없다. 기록을 지우면 닫을 길이 없어진다.
    rc.world.lock().unwrap().hidden = true;
    status(&rc).await;
    assert!(record(&rc).exists(), "살아 있는 pid 의 기록은 지킨다");
    rc.world.lock().unwrap().hidden = false;
    assert_eq!(status(&rc).await["handoffs"][0]["name"], NAME);

    // 프로브가 실패하면 서버가 죽었어도 지우지 않는다.
    {
        let mut w = rc.world.lock().unwrap();
        w.alive.clear();
        w.ps_fails = true;
    }
    status(&rc).await;
    assert!(record(&rc).exists(), "모르는 것으로는 지우지 않는다");

    // 프로브가 되고 정말 없으면 지운다.
    rc.world.lock().unwrap().ps_fails = false;
    status(&rc).await;
    assert!(!record(&rc).exists());
}

#[tokio::test]
async fn a_reused_pid_is_never_signalled() {
    let f = fx();
    let rc = rc_fixture(&f, none, plain);
    assert_eq!(spawn(&rc).await.0, 201);
    // 그 pid 가 이제 다른 폴더의 rc 서버다.
    rc.world.lock().unwrap().moved = true;
    let (code, body) = close(&rc, "rocky-todo-1", ReqOptions::default()).await;
    assert_eq!(code, 404, "{body}");
    assert!(error(&body).contains("손대지 않고"), "{body}");
    assert!(
        rc.world.lock().unwrap().alive.contains(&SERVER),
        "신호를 보내지 않았다"
    );
    assert!(!record(&rc).exists(), "틀린 기록은 지운다");
}
