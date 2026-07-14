---
description: 로키의 소울(페르소나 — 말투/성격 + 작업 방식)을 고른다. 목록 보기 / 활성 소울 변경(rocky.json 의 soul) / 미리보기 / 커스텀 소울 스캐폴딩. 프리셋은 souls/, 커스텀은 ~/.config/rocky/souls/.
argument-hint: "[list | <name> | show [name] | new <name>] [--project]"
allowed-tools: Read, Write, Edit, Bash
---

# soul — 로키 소울(페르소나) 선택

rocky 의 "소울" 은 말투/성격 + 작업 방식을 담은 페르소나다. 활성 소울은 `rocky.json` 의
`soul` 필드에 고정되고, `SessionStart` 훅이 매 세션 자동 주입한다(다음 세션부터 반영).
소울 파일: 프리셋 `souls/<name>.md` (번들), 커스텀 `~/.config/rocky/souls/<name>.md`.
`$ARGUMENTS` 로 서브커맨드를 받는다.

## 서브커맨드

- **(없음) 또는 `list`** — 사용 가능한 소울(프리셋+커스텀)을 나열하고 현재 활성 소울을 표시한다.
  - 프리셋 목록: `souls/*.md`. 커스텀 목록: `~/.config/rocky/souls/*.md` (있으면). 같은 이름이면 커스텀이 이긴다.
  - 각 파일의 frontmatter `name` / `description` 한 줄로 보여준다.
  - 현재 활성: `~/.config/rocky/rocky.json` 과 (있으면) `./rocky.json` 의 `soul` 필드(프로젝트 우선).

- **`<name>`** — 활성 소울을 `<name>` 으로 바꾼다.
  1. 해당 이름의 소울이 프리셋/커스텀에 실제 있는지 먼저 확인한다. 없으면 목록을 보여주고 멈춘다.
  2. 대상 파일: 기본 `~/.config/rocky/rocky.json`(user), `--project` 면 `./rocky.json`(project).
  3. **쓰기 전 사용자에게 확인**한다. 승인 후, 대상 JSON 을 읽어 `soul` 키만 갱신한다(다른 필드 보존, 파일 없으면 `{ "soul": "<name>" }` 로 생성).
  4. 완료 후 "다음 세션부터 적용됨" 을 알린다.

- **`show [name]`** — 소울 본문(페르소나 전문)을 미리 보여준다. 이름 생략 시 현재 활성 소울.

- **`new <name>`** — 커스텀 소울을 스캐폴딩한다.
  1. `~/.config/rocky/souls/<name>.md` 가 이미 있으면 덮어쓰지 않고 경고 후 멈춘다.
  2. 없으면 아래 템플릿으로 생성하고, 사용자가 본문을 채우도록 안내한다:
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
  3. `--project` 는 여기선 무시(커스텀 소울은 user 디렉터리에만 산다). `soul` 로 활성화하려면 `/rocky:soul <name>`.

## 원칙

- 소울은 AGENTS.md 게이트/안전 규칙 위의 레이어일 뿐 — 그 규칙을 덮어쓰지 않는다.
- 이름은 `[a-zA-Z0-9_-]+` 만 (파일명/`soul` 필드 제약과 동일).
- `rocky.json` 을 쓸 때 기존 필드를 보존한다 — `soul` 키만 건드린다.
