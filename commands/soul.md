---
description: 로키의 소울(페르소나 — 말투/성격 + 작업 방식)을 고른다. 목록 보기 / 활성 소울 변경(rocky.json 의 soul) / 호칭 설정(callsign) / 미리보기 / 커스텀 소울 스캐폴딩. 프리셋은 ${CLAUDE_PLUGIN_ROOT%/}/souls/, 커스텀은 ~/.config/rocky/souls/.
argument-hint: "[list | <name> | call [<이름>|--clear] | show [name] | new <name>] [--project]"
allowed-tools: Read, Write, Edit, Bash(ls:*)
---

# soul — 로키 소울(페르소나) 선택

rocky 의 "소울" 은 말투/성격 + 작업 방식을 담은 페르소나다. 활성 소울은 `rocky.json` 의
`soul` 필드에 고정되고, `SessionStart` 훅이 매 세션 자동 주입한다(다음 세션부터 반영).
소울 파일: 프리셋 `${CLAUDE_PLUGIN_ROOT%/}/souls/<name>.md` (번들), 커스텀
`~/.config/rocky/souls/<name>.md`. `$ARGUMENTS` 로 서브커맨드를 받는다.

> `${CLAUDE_PLUGIN_ROOT}` 는 이 플러그인이 설치된 루트 디렉터리로, 커맨드 본문 로드 시점에
> 자동 치환되고 Bash 서브프로세스에도 env 로 노출된다. 사용자의 현재 cwd 와 무관하게 항상
> 번들 프리셋 위치를 정확히 가리키므로, 프리셋 참조는 반드시 이 경로를 쓴다(bare `souls/` 는
> 레포 밖에서 실행하면 찾지 못한다). `%/` 는 후행 슬래시를 정규화한다 — 루트에 슬래시가
> 있든(`.../rocky/`) 없든(`.../rocky`) `${CLAUDE_PLUGIN_ROOT%/}/souls` 는 슬래시 하나로만
> 이어진다. 프리셋 파일을 Read 로 열기 전, 존재 확인은 `ls "${CLAUDE_PLUGIN_ROOT%/}/souls/<name>.md"`
> 로 하고 그 출력에 찍힌 실제 절대경로를 Read 에 넘긴다(Read 는 shell 확장을 하지 않는다).

## 서브커맨드

- **(없음) 또는 `list`** — 사용 가능한 소울(프리셋+커스텀)을 나열하고 현재 활성 소울을 표시한다.
  - 프리셋 목록: `ls "${CLAUDE_PLUGIN_ROOT%/}/souls"` 로 나열한 `*.md`. 커스텀 목록:
    `ls ~/.config/rocky/souls` 로 나열한 `*.md` (있으면). 같은 이름이면 커스텀이 이긴다.
  - 각 파일의 frontmatter `name` / `description` 한 줄로 보여준다.
  - 현재 활성: `~/.config/rocky/rocky.json` 과 (있으면) `./rocky.json` 의 `soul` 필드(프로젝트 우선).

- **`<name>`** — 활성 소울을 `<name>` 으로 바꾼다.
  1. **먼저** `<name>` 이 `^[a-zA-Z0-9_-]+$` 를 만족하는지 검증한다. 아니면 사용법을 보여주고 즉시 멈춘다(파일 경로를 구성하지 않는다).
  2. 해당 이름의 소울이 프리셋(`${CLAUDE_PLUGIN_ROOT%/}/souls/<name>.md`)/커스텀(`~/.config/rocky/souls/<name>.md`)에 실제 있는지 확인한다. 없으면 목록을 보여주고 멈춘다.
  3. 대상 파일: 기본 `~/.config/rocky/rocky.json`(user), `--project` 면 `./rocky.json`(project).
  4. **호칭을 묻는다** — "소울이 뭐라고 불러 드릴까요?" 현재 `callsign` 이 설정돼 있으면 보여주고 유지/변경/생략 중 고르게 한다. 응답이 있으면 호칭 규칙(한 줄, 공백만은 불가, 최대 40자 — 한글/공백 OK)을 검증하고, 어긋나면 다시 묻는다. 생략하면 `callsign` 은 건드리지 않는다(소울 본문의 기본 호칭 사용).
  5. **쓰기 전 사용자에게 확인**한다. 승인 후, 대상 JSON 을 읽어 `soul` 키(+ 4에서 호칭을 받았다면 `callsign` 키)만 갱신한다(다른 필드 보존, 파일 없으면 새로 생성).
  6. 완료 후 "다음 세션부터 적용됨" 을 알린다.

- **`call [<이름>] [--clear]`** — 소울이 사용자를 부르는 호칭(`rocky.json` 의 `callsign`)만 다룬다. 소울 자체는 바꾸지 않는다.
  - **인자 없음** → 현재 호칭을 보여준다: `~/.config/rocky/rocky.json`(user) 과 (있으면) `./rocky.json`(project) 각각의 `callsign`, 그리고 병합 결과(project 우선)를 명시한다.
  - **`<이름>`** → 호칭 규칙(한 줄, 공백만은 불가, 최대 40자 — 한글/공백 OK, `soul` 과 달리 `[a-zA-Z0-9_-]` 제약 없음)을 검증한다. 어긋나면 사용법을 보여주고 멈춘다. 대상 파일은 기본 user, `--project` 면 project. **쓰기 전 확인** 후 대상 JSON 의 `callsign` 키만 갱신한다(다른 필드 보존, 파일 없으면 `{ "callsign": "<이름>" }` 로 생성).
  - **`--clear`** → **확인 후** 대상 JSON 에서 `callsign` 키를 제거한다(다른 필드 보존).
  - 완료 후 "다음 세션부터 적용됨" 을 알린다 (SessionStart 훅이 주입).

- **`show [name]`** — 소울 본문(페르소나 전문)을 미리 보여준다. 이름 생략 시 현재 활성 소울.
  1. `name` 이 주어졌다면 **먼저** `^[a-zA-Z0-9_-]+$` 를 만족하는지 검증한다. 아니면 사용법을 보여주고 즉시 멈춘다(파일 경로를 구성하지 않는다).
  2. 프리셋(`${CLAUDE_PLUGIN_ROOT%/}/souls/<name>.md`) 또는 커스텀(`~/.config/rocky/souls/<name>.md`)에서 파일을 읽어 본문을 보여준다(같은 이름이면 커스텀이 이긴다).

- **`new <name>`** — 커스텀 소울을 스캐폴딩한다.
  1. **먼저** `<name>` 이 `^[a-zA-Z0-9_-]+$` 를 만족하는지 검증한다. 아니면 사용법을 보여주고 즉시 멈춘다(파일 경로를 구성하지 않는다).
  2. `~/.config/rocky/souls/<name>.md` 가 이미 있으면 덮어쓰지 않고 경고 후 멈춘다.
  3. 없으면 아래 템플릿으로 생성하고, 사용자가 본문을 채우도록 안내한다:
     ```markdown
     ---
     name: <name>
     description: <한 줄 설명>
     ---

     ## 말투 / 성격
     - ...

     ## 작업 방식
     - ...
     ```
  4. `--project` 는 여기선 무시(커스텀 소울은 user 디렉터리에만 산다). `soul` 로 활성화하려면 `/rocky:soul <name>`.

## 원칙

- 소울은 AGENTS.md 게이트/안전 규칙 위의 레이어일 뿐 — 그 규칙을 덮어쓰지 않는다.
- 소울 이름은 `[a-zA-Z0-9_-]+` 만 (파일명/`soul` 필드 제약과 동일). 호칭(`callsign`)은 파일명이 아니므로 한글/공백 OK — 한 줄, 공백만은 불가, 최대 40자만 지킨다.
- `rocky.json` 을 쓸 때 기존 필드를 보존한다 — `soul` / `callsign` 키만 건드린다.
