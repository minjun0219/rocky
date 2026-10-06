//! rocky CLI — 데몬의 얇은 HTTP 클라이언트 (보조 표면).
//!
//! 에이전트의 주 경로는 데몬의 `/mcp` 지만, CLI 는 사람/스크립트/데몬 관리용으로
//! 전체 동작을 커버한다. 출력은 컴팩트 텍스트 한 줄주의 — `--json` 으로 원본 JSON.
//!
//! 순수 계층(파싱·포맷)을 lib 으로 열어 두는 이유는 테스트가 붙게 하기 위해서다.

pub mod bounded;
pub mod channel;
pub mod client;
pub mod commands;
pub mod config_cmd;
pub mod context;
pub mod flags;
pub mod format;
pub mod git_status;
pub mod hooks;
pub mod launchd;
pub mod rc_cmd;
pub mod statusline_cache;
pub mod statusline_doctor;
pub mod statusline_guard;
pub mod statusline_refresh;
pub mod system;
pub mod tokens_cmd;
pub mod usage_cmd;
pub mod verify_cmd;
pub mod worklog_mcp;

/// `help` 출력 — TS `src/cli.ts` 의 HELP 를 그대로 옮겼다.
pub const HELP: &str = r#"rocky — 공유 todo/스크래치패드 보드 (데몬 + MCP 의 CLI 표면)

사용:
  rocky ls [--board K|--all] [--archived] [--json]
  rocky next [--board K|--all] [--limit N] [--json]   착수 후보 랭킹 (다음에 뭘 할까)
  rocky today [--json]                    보드 요약 몇 줄 (마감·진행중·핸드오프·수집함 제목) — `! rocky today`
  rocky inbox [--json]                    수집함 소스별 항목 (✓ 보드에 올라감 · 실패 사유) — `! rocky inbox`
  rocky inbox subscribe <소스> | unsubscribe [소스]   이 세션이 소스를 구독 — 새 항목을 세션에 알린다(착수는 사람이)
  rocky add "제목" [--board K] [--section S] [--parent REF] [--desc MD]
                       [--due YYYY-MM-DD] [--priority p1..p4] [--label a,b] [--link URL]
  rocky show REF · edit REF [플래그] [--title "새 제목"]
  rocky comment REF "본문"                 todo 에 댓글 (에이전트/사람 공용 타임라인)
  rocky issue REF [--repo OWNER/NAME]      todo 를 GitHub 이슈로 (gh CLI 필요)
  rocky handoff REF [--session NAME] [--message "본문"]  실행 중인 세션에 작업 요청 보내기
  rocky handoff REF --cancel               대기 중인 요청 취소
  rocky spawn REF [--message "본문"]        그 todo 전용 워크트리에 새 세션 띄우기
  rocky sessions                           실행 중인 Claude Code 세션 (* = 이 보드)
  rocky pr [--board K|--all] [--json]      구독한 열린 PR — 확인·머지 가능 / 충돌 / 대기
  rocky pr subscribe|unsubscribe N [--repo OWNER/NAME] · pr subscriptions
                                          데몬이 이 PR 을 보고 전이를 이 세션에 보낸다(세션 밖이면 지켜보기만)
  rocky pr subscribe --filter "repo:o/r author:@me" · pr unsubscribe --filter ID
                                          GitHub 검색 조건에 걸리는 열린 PR 을 이 세션이 받는다
  rocky move REF --to BOARD | --before REF2 | --last   보드 이동 / 순서 이동
  rocky start|stop|done|reopen|archive|unarchive REF
  rocky section add|archive "이름" [--board K] · section ls [--board K]
  rocky note add "제목" [--board K|--global] [--content MD]
  rocky note ls [--board K|--global]
  rocky note show REF [--global] | edit REF --content MD [--global] |
                       append REF "텍스트" [--global] | archive REF [--global] |
                       pin|unpin REF [--global]   (고정 — 웹 노트 탭 맨 위에 펼쳐 둔다)
  rocky history REF [--limit N] [--global|--note] · section ls
  rocky board ls|show|add|rename|title|desc|repo|path|review-fix|pr-authors   보드 메타 (이름·slug·설명·GitHub·리뷰 반영·알릴 PR 작성자)
  rocky open                              접속 주소 출력 (로컬/내부망/테일넷 — 링크 클릭으로 열기)
  rocky daemon run|start|stop|restart|status|install|uninstall   restart 는 버전과 상관없이 지금 설치본으로 교체, status 는 launchd 로드 여부와 고치는 명령까지
  rocky version | --version               설치된 CLI 버전
  rocky update [--check]                  플러그인·데몬을 최신 릴리스로 (--check 는 버전 비교만)
  rocky statusline [--cwd P] [--session S]  rocky 한 줄 — 없으면 stdin 의 Claude Code 입력을 읽는다
  rocky statusline --full [--source S]      경로·git·모델·한도 줄(cc-usage 와 같은 출력) + 보드 줄 — stdin 을 읽는다.
                                            S = auto|stdin|api|none(이 실행에서만). agy 의 statusLine 에도 같은 명령
  rocky statusline guard                    UserPromptSubmit 훅 — 한도 소진으로 크레딧이 차감되면 prompt 를 막는다(exit 2).
                                            statusline.guard 가 켜져 있어야 돈다
  rocky statusline allow [DURATION|off]     guard 를 잠시 끈다(기본 30m, 예: 2h · 1h30m). off 면 다시 켠다
  rocky statusline probe                    usage API 원본 응답(필드 확인용)
  rocky statusline doctor [--session S]     설정·계정·토큰·캐시·keychain 후보·extraCommands 진단
  rocky config show|init|link|path [--json] 설치·설정 점검 / 기본 rocky.json 생성 / ~/.local/bin/rocky 링크 / 설정 파일 경로
  rocky usage [--since 30d] [--json]        사용 로그 보고 — 많이 쓴 표면 · 에러 · 안 쓴 표면 (rocky.json usage 블록으로 끔)
  rocky tokens [--since 30d] [--by model,effort|model|effort|session|branch] [--json]
                                          Claude Code 토큰 합계 — 모델·effort 고를 때 참고 (rocky.json tokens 블록)
  rocky tokens here [--cwd P] [--json]    이 디렉터리의 최근 세션 — 턴별 모델·effort·토큰과 추천
  rocky verify [--json]                   기본 브랜치 검증 — 대상마다 마지막 결과(rocky.json verify 블록)
  rocky verify --rerun [BOARD] [--branch B]  같은 커밋을 다시 검증(거짓 실패 풀기) — 보드를 안 주면 대상 전부
  rocky rc [status] [--activity] [--json] claude rc 서버 현황(rocky.json 의 rc 대상 · 대상 밖 서버 · 자격)
                                          --activity 는 대상마다 최근 활동(git — 대상 수만큼 걸린다)
  rocky rc agy [start|stop]               Antigravity 원격 제어 보기 · 켜기 · 끄기(로컬 전용)
  rocky rc start <라벨> [--wait] · rc start --all · rc restart <라벨> [--fresh | --session <cse_…>] [--wait]
                                          데몬이 띄우거나 다시 띄운다(--all 은 꺼진 대상 전부를 서버만으로).
                                          재시작은 붙은 원격 세션을 끊는다 — 막 대화하는 중이면 턴이 끝날
                                          때까지 기다리고, 이 세션이 붙은 서버는 거절한다
  rocky rc nightly [--dry-run]            야간 재시작을 지금 한 번(--dry-run 은 판정만)
  rocky rc report [--json]                마지막 야간 보고 — 서버마다 결과, rocky · agy 버전
  rocky rc stop <할 일 | 대상 밖 서버>      핸드오프 서버(할 일 참조)나 대상 밖 서버(이름 · pid)를 닫는다(폴더는 남긴다)
  rocky mcp setup                         호스트별 MCP 등록 안내
  rocky mcp worklog [--roots]             worklog_* 4 도구 stdio MCP 서버 (플러그인이 띄운다; --roots 는 프로젝트를 클라이언트 roots 로)
  rocky tailscale on|off|status           테일넷 한정 HTTPS 노출 (옵션, 기본 off)

REF 는 12 (현재 보드) 또는 rocky-12 (보드 지정) 또는 raw id 를 받는다.
보드 키는 생략 시 cwd 의 git repo 이름으로 유추한다. actor 는 --actor >
ROCKY_TODO_ACTOR > 호스트 자동 감지. 삭제는 없다 — 아카이브만 존재한다.
note show/edit/append/archive 의 맨 번호(12)는 기본적으로 todos 와 동일하게 현재 보드
컨텍스트로 풀린다 — 전역 메모를 번호로 가리키려면 note-3 처럼 접두사를 붙이거나
--global 을 붙인다. 둘 다 없으면 같은 번호의 보드 메모가 대신 잡힐 수 있다(모호성 회피).
옛 표기(rocky#12 / #12)도 계속 받는다 — 다만 bash 에서 #12 는 주석 시작 문자라
따옴표가 필요하다: rocky show '#12'"#;
