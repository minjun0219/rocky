# rc 서버 관리 1조각 — 현황 보기(읽기 전용) 실행 계획

> **날짜**: 2026-10-05 · **보드**: rocky-28 · **설계**: [`specs/2026-10-05-rc-server-design.md`](../specs/2026-10-05-rc-server-design.md)

1조각은 rc 서버를 **보기만** 한다. 띄우기·내리기·잠금·상태 쓰기는 하나도 없다. 그래서 옛 CLI와 같이 돌아도 서로 부딪치지 않는다.
설계 8절 5번(웹은 요약 한 줄과 전용 화면 둘 다)을 넣으면 400줄을 넘기므로 PR을 둘로 나눈다.

| PR | 내용 | 예상 크기(테스트 제외) |
|---|---|---|
| 1a | 설정 `rc` 블록 · `rocky_core::rc` 판정 · 프로브 · `GET /api/rc/servers` · `rocky rc` | 350~400줄 |
| 1b | 웹: "지금" 위 요약 한 줄 + rc 탭 | 250~300줄 |

1b는 1a의 응답 모양에 기대므로 `gh stack`으로 쌓는다.

## 1a — 코어·REST·CLI

### 설정 (`config.rs` + `rocky.schema.json`)

이번 조각은 목록과 `enabled`(기능 전체 스위치, 기본 true — rc 를 못 쓰는 기기에서 끈다)만 읽는다. `supervise`·`nightly`는 쓰는 조각(3·4)에서 넣는다.

```jsonc
"rc": { "root": "~/dev/workspaces", "pinned": ["repo-a"], "targets": ["repo-b"] }
```

- `RcConfig { root: Option<String>, pinned: Vec<String>, targets: Vec<String> }`, `load_rc_block`(fail-open, `load_pr_block`과 같은 모양).
- 사용자 `rocky.json`에서만 읽는다. 전역 데몬이라 `todo` 블록과 규칙이 같다.
- 항목 해석은 옛 CLI 형식을 따른다. `/`로 시작하면 절대경로, `~`로 시작하면 홈 기준, 나머지는 `root` 아래로 본다. 라벨은 디렉터리 basename이다.

### 순수 판정 (`crates/rocky-core/src/rc.rs`)

| 함수 | 입력 → 출력 | 고정할 함정(설계 6절 번호) |
|---|---|---|
| `is_server_argv(&[&str])` | argv → bool | 2(셸 래퍼 오인), 3(`--name` 없는 일회성 호출) |
| `parse_ps(&str)` | `ps -axww -o pid=,etime=,args=` 출력 → `Vec<PsRow{pid, elapsed_secs, argv}>` | etime 로케일 무관 파싱 |
| `parse_lsof_cwd(&str)` | `lsof -F pn` 출력 → `HashMap<pid, cwd>` | |
| `has_live_session(&[String])` | 자식 명령줄들 → `--sdk-url …/code/session` 이 있나 | |
| `resolve_targets(&RcConfig, home)` | 설정 → `Vec<Target{label, dir, pinned}>`(중복 제거, pinned 우선) | |
| `parse_auth_status(&str)` | `claude auth status --json` → `In / Out / Unknown` | 9(모르면 Unknown, 막지 않음) |
| `parse_agy_status(&str)` | `agy remote-control status` → `{state, pid, instance}` | |
| `build_status(targets, servers, now)` | 판정 결과를 합친다 → 대상별 행 + "대상 밖 서버" 행 | 15(개명) |

- 맞대기는 옛 CLI처럼 **cwd 문자열이 같은지**로 한다. 정규화하지 않는다. 다르게 하면 동등성 비교가 흐려진다.
- "구버전" 표시는 버전 기록이 있어야 한다. 기록은 기동 때 쓰므로 2조각으로 미룬다.

### 프로브 (`crates/rockyd/src/rc.rs`)

- 호출 순서: `ps` 한 번 → 서버 pid 전체로 `lsof` 한 번 → 서버마다 `pgrep -a -P` + `ps -o command=`(함정 4) → `claude auth status --json` → `agy`(PATH에 없으면 건너뜀).
- 실행은 전부 기존 `runner`(타임아웃 + kill_on_drop)로 한다. 보기만 하는 짧은 명령이라 장수 프로세스 경로가 필요 없다.
- 결과는 5초 TTL로 캐시한다. 세션 목록 캐시와 같은 모양이고, 화면 폴링이 `ps`/`lsof`를 반복해서 부르지 않게 하려는 것이다.
- `rc` 블록이 없으면 프로브를 돌리지 않고 빈 응답을 낸다. 다른 기기에서는 비용이 0이다.

### REST

- `GET /api/rc/servers` — 열림(읽기 전용). 응답:

```jsonc
{
  "configured": true,
  "servers": [
    { "label": "repo-a", "dir": "/…/repo-a", "pinned": true, "running": true,
      "pid": 123, "startedAt": "…", "session": true, "authSuspect": false }
  ],
  "strays": [ { "label": "old-name", "dir": "/…", "pid": 456, "startedAt": "…" } ],
  "auth": "in",              // in | out | unknown
  "antigravity": { "installed": true, "state": "running", "instance": "…" }  // 없으면 null
}
```

- `KNOWN_SURFACES`에 라우트를 넣는다.

### CLI

- `rocky rc`(= `rocky rc status`): 대상별 한 줄(●/○ · 라벨 · 고정 · 세션 · 기동 시각), 맨 끝에 자격 한 줄과 Antigravity 한 줄. `--json`은 응답을 그대로 낸다.
- 하위 명령은 지금은 `status` 하나다. `start`·`restart`는 2조각에서 넣는다.

### 테스트

- `crates/rocky-core/tests/it/rc_test.rs`: 위 표의 함수마다 실제 출력에서 뜬 픽스처로 테스트한다. 셸 래퍼, `claude rc --help`, `--name=` 형식, 공백 든 경로, 대상 밖 서버를 넣는다.
- `crates/rockyd/tests/it/rc_route_test.rs`: 가짜 runner를 주입해 라우트 응답 모양과 `rc` 블록 없음을 확인한다.

### 문서·릴리스

- `README.md`: CLI·라우트·설정 표, `AGENTS.md`: *데몬·설치 모델*에 한 줄("rc 서버 현황은 읽기만 하고, 띄우기는 다음 조각").
- `rocky.schema.json` + `config.rs`를 함께 바꾼다(변경 체크리스트 5).
- `bunx changeset`(minor).

## 1b — 웹

- `web/DESIGN.md`를 먼저 읽는다. 아래는 그 규칙을 따르는 안이다.
- **요약 한 줄**: "지금" 표 위에 `rc 9/10 · 세션 3`을 둔다. 고정 서버가 꺼졌거나 자격이 out이면 그 사실만 상태 색으로 보인다. 누르면 아래로 서버 목록이 펼쳐진다. `rc` 블록이 없으면 줄 자체가 없다.
- **rc 탭**: GitHub 탭처럼 보드에 묶이지 않는 전역 화면이다. ⋯ 메뉴에서 숨길 수 있다.
  - 한 열 목록: 대상 행 → "대상 밖 서버" → 자격 한 줄 → Antigravity 한 줄.
  - 버튼은 아직 없다(2조각).
- 갱신: 탭이나 펼침이 보일 때만 30초 간격으로 폴링한다. SSE는 띄우기가 생기는 2조각에서 다시 판단한다.
- `types.ts`에 응답 타입 사본, 컴포넌트 옆 `*.test.tsx`, Playwright로 360·860·1440px × 라이트·다크.
- e2e 픽스처 데몬에는 `rc` 블록이 없다. 그래서 "줄 없음"만 확인하고, 목록 화면은 DOM 테스트로 본다.

## 하지 않는 것(이 조각)

- 서버를 띄우거나 내리는 모든 것, 버전 기록, 되살림 표식, 이벤트 로그, `rc.supervise`.
- MCP 도구(설계 4절).

## 확인

1. 게이트 6개(`bun run check`/`typecheck`/`test`, `cargo fmt`/`clippy`/`test`).
2. 이 맥에서 개발 데몬(`ROCKY_CONFIG=<전용 파일> cargo run -p rockyd`)에 실제 목록을 넣고 `rocky rc`를 돌린다. 출력을 옛 CLI의 현황(`-s`)과 대상별로 맞대어 실행 여부·세션 여부·대상 밖 서버가 같은지 본다.
