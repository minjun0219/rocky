# Codex 서브에이전트 (감시형 위임) + 첫 실사용으로 codex-plugin 구현 (design)

- 날짜: 2026-07-14
- 브랜치: `minjun0219/codex-plugin`
- 한 줄: rocky 에 **Codex 를 구현자 서브에이전트로 위임하고 Claude 가 감독하는 `/codex` 슬래시 커맨드**를 만들고, 그 첫 실사용으로 **"rocky 를 Codex 에서 쓰게 하는 문서"를 Codex 가 직접 구현**하게 한 뒤 Claude 가 검증한다.

## 정정된 요구 (owner 확인 완료)

이전에 나(Claude)는 이 요청을 "rocky-in-codex 문서를 내가 직접 쓴다"로 오독했다. 실제 의도는 **오케스트레이션**이다:

1. **먼저** Codex 를 구현자 서브에이전트로 dispatch 하는 장치를 **만든다**(재사용 가능).
2. **그 서브에이전트(=Codex)가** "코덱스 플러그인"(rocky-in-codex 통합)을 **스스로 구현**한다.
3. **Claude 는 그걸 감시** — Codex 의 작업이 rocky(Claude Code) 플러그인 동작을 깨지 않는지 감독하고, 깨끗할 때만 병합, 아니면 에스컬레이션.

즉 rocky-in-codex 는 *결과물*이고, 방법은 "Claude 가 직접 짜지 않고 Codex 에게 시키고 Claude 는 감독"이다.

## 확정 결정 (owner 확인 완료)

- **서브에이전트 형태**: rocky **슬래시 커맨드** `/codex` (commands/). `/finish`·`/curate` 와 같은 host-LLM 오케스트레이션 패턴, MCP 도구 아님. 순수 markdown(신규 TS 런타임 코드/deps 없음).
- **격리**: Codex 는 **새 git worktree** 에서 작업. Claude 가 diff + 게이트 통과 확인 후 병합. 현재 작업트리 보호, 되돌리기 쉬움.
- **라이브 실행**: 이번 세션에서 하네스를 만들고 **실제로 Codex 를 돌려** codex-plugin 문서까지 구현·검증.

## 실현 가능성 (확인됨)

- `codex` CLI `0.44.0` 설치됨(`/opt/homebrew/bin/codex`).
- `codex exec` 비대화형 실행이 감시에 필요한 걸 모두 제공:
  - `-C, --cd <DIR>` 작업 루트(worktree 격리)
  - `-s read-only|workspace-write|danger-full-access`, `--full-auto`(= `-a on-failure --sandbox workspace-write`)
  - `--json` JSONL 이벤트 스트림(실시간 감시), `--output-last-message <FILE>` 최종 메시지 캡처
  - `--output-schema <FILE>` 구조화 최종 응답, `--skip-git-repo-check`
- rocky 의 `src/index.ts` 는 이미 호스트 무관 stdio MCP 서버 → Codex 가 짤 "rocky-in-codex" 산출물은 **런타임 코드 변경 0**, `~/.codex/config.toml` 스니펫 + 문서만(아래 부속 스코프 참고).

## 아키텍처

### 1) `/codex` 슬래시 커맨드 (Claude 가 이번에 직접 구축)

`commands/codex.md` — Claude 가 따르는 host-LLM 절차. 입력: task 설명(`$ARGUMENTS`).

절차:

1. **격리 준비**: 현재 브랜치에서 갈라진 새 git worktree 를 만든다 (`git worktree add <path> -b codex/<task-slug>`).
2. **위임(dispatch)**: 아래를 실행해 Codex 에게 구현을 맡긴다.
   ```
   codex exec --full-auto -C <worktree> \
     --json --output-last-message <worktree>/.codex-last.txt \
     "<task + 가드레일 프롬프트>"
   ```
   가드레일 프롬프트에 rocky 불변식을 못박는다: (a) MCP 도구 표면(도구 개수/이름)을 바꾸지 말 것, (b) 게이트(`bun run check` / `typecheck` / `bun test`)를 통과시킬 것, (c) 요청 스코프 밖 파일을 건드리지 말 것, (d) 두 단일 소스(FEATURES.md 한글 / AGENTS.md 영문) 동기화 규칙 준수.
3. **감시(supervise)**: Codex 종료 후(필요 시 진행 중 스트림도) 다음을 Claude 가 직접 검증한다 — 이것이 "Claude Code 플러그인 작동을 방해하지 않는지 확인" 의 구체 정의:
   - **게이트**: worktree 에서 `bun run check`, `bun run typecheck`, `bun test` 실행 → 모두 통과.
   - **MCP 표면 무결성**: `bun test src/index.test.ts` (in-memory smoke) 로 도구 개수 + `REMOVED_TOOLS` 누수 가드 + `JOURNAL_TOOLS` 상시 등록을 확인. 표면이 바뀌면 "플러그인 작동 방해"로 간주.
   - **플러그인 선언 무결성**: `.claude-plugin/plugin.json` 의 `mcpServers` 가 파손되지 않았는지.
   - **diff 스코프**: 변경이 의도한 파일 집합(이번 task 는 문서)에 한정됐는지, 예상치 못한 런타임 코드 변경이 없는지.
4. **판정/에스컬레이션**:
   - 깨끗 → 변경 요약 + diff 를 owner 에게 제시하고 현재 브랜치로 병합(또는 `/finish` 흐름 연계). **자동 머지 없음**(rocky 정책: pr-watch 와 동일).
   - 문제 → 무엇을 깼는지(게이트/표면/스코프) 보고하고 **병합하지 않음**. 필요 시 가드레일을 보강해 Codex 에 재위임하거나 owner 에스컬레이션.

원칙: Claude 는 **감독자**, Codex 는 **구현자**. Claude 가 직접 구현 코드를 쓰지 않는다(감시·게이트·판정만). 자동 병합·자동 push 없음.

### 2) 첫 실사용 task (Codex 가 구축, Claude 감독)

Codex 에 넘길 task = **"rocky 를 Codex 에서 쓸 수 있게 (문서/설정)"**. 부속 스코프:

- 신설 `docs/codex.md` (`docs/openapi-mcp.md` 대칭 보조 문서): `~/.codex/config.toml` 등록 스니펫
  ```toml
  [mcp_servers.rocky]
  command = "bun"
  args = ["run", "/abs/path/to/rocky/src/index.ts"]
  ```
  + `codex mcp add rocky -- bun run <abs>/src/index.ts` + 주의점(`cwd` 의존: rocky.json project scope & journal 프로젝트 키가 cwd 기반 / `env` 로 `ROCKY_JOURNAL_DIR` 등 오버라이드 / `bun` PATH 전제 / notion 은 `ntn` 탐지 시만 / 슬래시커맨드·스킬은 CC 전용이라 비노출).
- `FEATURES.md`: Codex 사용 섹션(한글).
- `AGENTS.md`: *Project in one line* + 배포 서술을 "2 배포 타깃 → 전체 표면 서버(`src/index.ts`) + 이를 소비하는 N 호스트(CC 플러그인 + Codex)" 로 리프레이밍, Layout 에 `docs/codex.md` 추가.
- `README.md`: Codex 소비 호스트 한 줄.
- 런타임 코드/`plugin.json`/`rocky.schema.json`/`package.json` 변경 없음.

### 실행 순서 (drift/충돌 방지)

1. Claude: `/codex` 커맨드 하네스 + 그에 대한 FEATURES/AGENTS/README 문서화 작성 → **커밋**.
2. Claude: 커밋된 현재 브랜치에서 worktree 를 갈라냄(하네스가 포함된 상태).
3. Codex: worktree 안에서 rocky-in-codex 문서 task 구현.
4. Claude: 감시(게이트/표면/스코프) → 판정 → 병합 or 에스컬레이션.

이 순서면 Codex 의 문서 변경(FEATURES/AGENTS/README)이 Claude 의 하네스 커밋 위에 얹혀 충돌 없음.

## 변경 파일

**Claude 가 만드는 하네스 (branch `minjun0219/codex-plugin`)**

| 파일 | 변경 |
| --- | --- |
| `commands/codex.md` (신설) | `/codex <task>` — worktree 격리 → `codex exec` 위임 → 감시(게이트/표면/스코프) → 판정. 순수 markdown. |
| `FEATURES.md` | `/codex` 슬래시 커맨드 항목(한글) 추가. |
| `AGENTS.md` | Layout(commands/ 에 codex.md) + *Project in one line*(슬래시 커맨드 목록에 /codex, host-LLM 위임 오케스트레이션) 갱신. |
| `README.md` | 슬래시 커맨드 목록에 `/codex` 반영. |

**Codex 가 만드는 산출물 (worktree, 감독 하)** — 위 "첫 실사용 task" 파일 집합.

## 검증 (하네스 자체 + 라이브 실행)

- 하네스: `bun run check` / `typecheck` / `bun test` 통과(markdown 추가라 회귀 없어야 함). `/codex` 커맨드 형식이 CC 슬래시 커맨드 규약(front-matter/`$ARGUMENTS`)에 맞는지.
- 라이브 실행 성공 기준: Codex 가 만든 worktree 에서 게이트 3종 통과 + `src/index.test.ts` 표면 무결 + diff 가 문서에 한정 + 등록 스니펫이 실제로 `bun run src/index.ts` 를 stdio 로 띄워 `tools/list` 왕복(최소 openapi 7 + seo 1 + journal 4 = 12 도구, ntn 시 notion 4 추가)으로 확인.

## 비대상 (out of scope)

- Codex 의 자동 병합 / 자동 push(감독 후 owner 승인).
- `/codex` 를 MCP 도구나 standalone CLI 로 노출(슬래시 커맨드만).
- Codex-side 마켓 배포 / npm publish 자동화.
- rocky-in-codex 를 위한 런타임 코드·bin(`rocky-mcp`)·config shape 신설(문서/설정만, `src/index.ts`+`rocky.json` 재사용).
- 슬래시 커맨드/스킬의 Codex custom-prompts 포팅.

## 리스크 / 완화

- **Codex 가 스코프 밖(런타임 코드)을 건드림** → 가드레일 프롬프트 + diff 스코프 감시 + 게이트로 차단, 문제 시 병합 안 함.
- **Codex 가 게이트를 못 맞춤** → 병합 보류, 가드레일 보강 재위임 or 에스컬레이션.
- **worktree 정리** → 병합/폐기 후 `git worktree remove` 로 정리.
- **`--full-auto` 권한** → workspace-write 샌드박스(worktree 범위)로 제한, `danger-full-access` 미사용.
