# 로키 소울 (Rocky Souls) — 설계

> 상태: 승인됨 (2026-07-15) · 다음 단계: 구현 플랜 작성
> 관련: [[AGENTS.md]] · `hooks/hooks.json` · `src/core/rocky-config.ts`

## 배경 / 동기

프로젝트 헤일메리의 로키처럼, rocky 플러그인을 쓸 때 **로키의 "소울"(성격/정체성)을 선택**할 수 있게 한다. 소울은 에이전트의 **말투/성격**과 **작업 방식/원칙**을 함께 담은 페르소나 프로필이다. 사용자가 프리셋 중 고르거나 자기 소울을 직접 정의할 수 있고, 한 번 고르면 config 에 고정되어 매 세션 자동 적용된다.

### 결정된 요구사항 (브레인스토밍 확정)

1. **소울의 정체**: 말투/성격 + 작업 방식/원칙 (둘 다). Claude Code 의 output-style 개념과 유사하나 rocky 자체 표면으로 구현.
2. **출처**: 프리셋 + 커스텀 둘 다.
3. **적용 방식**: `rocky.json` 에 활성 소울 고정 + `SessionStart` 훅이 매 세션 자동 주입. 전환은 `/rocky:soul` 커맨드.

## 범위

### In

- 소울 = markdown 파일 (frontmatter `name`/`description` + 본문 페르소나 지침).
- 프리셋 소울 3종 (`rocky` / `senior` / `terse`) — 플러그인에 번들.
- 커스텀 소울 — `~/.config/rocky/souls/<name>.md`. 같은 이름이면 커스텀이 프리셋을 덮어씀.
- `rocky.json` 신규 스칼라 필드 `soul: "<name>"` — 기존 project > user precedence 상속.
- `SessionStart` 훅으로 활성 소울 페르소나를 `additionalContext` 주입 (fail-open).
- `/rocky:soul` 슬래시 커맨드 (list / set / show / new).
- 순수 로직(`src/core/soul.ts`) ↔ IO 엔트리(`src/hooks/inject-soul.ts`) 분리 — 기존 log-turn/transcript 분리 패턴 답습.

### Out

- MCP 도구화 (`soul_*`) — 소울은 commands/hooks/skills 처럼 Claude Code 전용 표면, MCP tool surface 불변.
- Codex 적용 — Codex 는 MCP 도구만 소비하므로 소울은 Claude Code 에만 적용 (기존 커맨드/스킬과 동일).
- output-style 네이티브 기능으로의 배포 (별도 검토 대상, 이번엔 rocky 자체 훅 방식).
- 소울별 도구 구성 프리셋 (활성 MCP 도구 조합 변경) — 이번 범위 아님.
- 폴링/자동 소울 추천 — 명시적 선택만.

## 핵심 원칙

소울은 **AGENTS.md 의 게이트·안전 규칙 위에 얹히는 "플레이버 + 작업 스타일 레이어"**다. 그것들을 덮어쓰지 않는다. 주입 텍스트 앞에 이 우선순위를 명시하는 preamble 을 붙여, 페르소나가 게이트/검증/안전 규칙과 충돌할 경우 항상 후자가 이기도록 한다. 프리셋 본문도 AGENTS.md 규칙(간결함, 장문 리포트 금지 등)과 싸우지 않게 작성한다.

## 컴포넌트

### 1. 저장 구조

- **소울 파일 포맷**: markdown. 앞부분에 `---` 로 감싼 최소 frontmatter:
  ```markdown
  ---
  name: rocky
  description: 헤일메리 로키 — 따뜻·충직한 엔지니어 동료
  ---

  (페르소나 본문: 말투/성격 + 작업 방식/원칙)
  ```
  - frontmatter 파서는 **의존성 0** — `name:` / `description:` 라인만 읽는 최소 파서 (js-yaml 미사용).
- **프리셋 디렉터리**: `${pluginRoot}/souls/<name>.md` (레포 체크인). `pluginRoot` 는 `import.meta` 기반 경로 (절대 `__dirname` 금지).
- **커스텀 디렉터리**: `~/.config/rocky/souls/<name>.md`.
- **머지 규칙**: 이름이 같으면 커스텀이 프리셋을 덮어씀. `listSouls()` 는 두 디렉터리 합집합 반환 (각 항목에 `source: 'preset' | 'custom'` 표시).

### 2. 활성 소울 config

- `RockyConfig` 에 `soul?: string` 추가.
- 검증(`validateSoul`): 문자열, 이름 패턴(`[a-zA-Z0-9_-]+`, 기존 `ID_BODY` 재사용 가능).
- `ALLOWED_TOP_KEYS` 에 `'soul'` 추가.
- `mergeConfigs`: 스칼라이므로 project 가 있으면 user 를 덮어씀.
- `rocky.schema.json` 에 `soul` 필드 lockstep 추가.

### 3. 코어 로직 — `src/core/soul.ts`

DI 가능한 순수 로직 (테스트에서 디렉터리 주입):

- `resolveSoulName(config): string | undefined` — 활성 소울 이름.
- `listSouls(dirs): SoulSummary[]` — 프리셋+커스텀 머지, `{ name, description, source, path }`.
- `readSoul(name, dirs): Soul | null` — `{ name, description, body }` 또는 null (커스텀 우선).
- `buildSoulContext(soul): string` — preamble(우선순위 명시) + 본문을 감싼 주입 문자열.
- `createSoulResolverFromEnv(...)` 형태로 기본 디렉터리 해석 (pluginRoot, `~/.config/rocky/souls`) — env/DI 오버라이드 가능.

### 4. SessionStart 훅 — `src/hooks/inject-soul.ts`

- `hooks/hooks.json` 에 `SessionStart` 엔트리 추가:
  ```json
  "SessionStart": [{ "hooks": [{ "type": "command", "command": "bun run \"${CLAUDE_PLUGIN_ROOT}/src/hooks/inject-soul.ts\"" }] }]
  ```
- 동작: stdin SessionStart 페이로드 → `cwd` 추출 → 그 경로로 `loadConfig` → `resolveSoulName` → `readSoul` → `buildSoulContext` → 다음 JSON 을 stdout 출력:
  ```json
  { "hookSpecificOutput": { "hookEventName": "SessionStart", "additionalContext": "<persona>" } }
  ```
- **Fail-open**: 소울 미설정 / 파일 없음 / 파싱·config 오류 → 빈 출력(또는 no additionalContext), 세션 시작 절대 차단 안 함. 오류는 stderr 로만.
- 엔트리는 얇게 — 로직은 전부 `src/core/soul.ts`.

### 5. 슬래시 커맨드 — `commands/soul.md` (`/rocky:soul`)

- `/rocky:soul` (인자 없음): 사용 가능한 소울 목록(프리셋+커스텀) + 현재 활성 표시 + 한 줄 설명.
- `/rocky:soul <name>`: 활성 소울 변경 — `rocky.json` 의 `soul` 기록. 기본 user 스코프(`~/.config/rocky/rocky.json`), `--project` 로 프로젝트 `rocky.json`. **쓰기 전 확인 프롬프트**.
- `/rocky:soul show [name]`: 페르소나 전문 미리보기 (인자 없으면 활성 소울).
- `/rocky:soul new <name>`: `~/.config/rocky/souls/<name>.md` 를 템플릿으로 스캐폴딩 후 편집 유도. 기존 파일 있으면 덮어쓰지 않고 경고.
- 커맨드는 gh 불필요 — 호스트 LLM 이 파일 read/write 로 처리.

### 6. 프리셋 라인업

| 이름 | 성격 | 작업 스타일 |
|------|------|-------------|
| `rocky` | 헤일메리 로키 — 따뜻·충직한 엔지니어 동료 ("good good") | 게이트 먼저, 완료 주장 전 검증, 간결한 한국어 |
| `senior` | 진지한 시니어, 군더더기 없음 | 트레이드오프 우선 제시, 근거 있는 반대(push back) |
| `terse` | 최소한의 말 | 답부터, 서론 없음 |

- 기본값: `soul` 미설정 시 **주입 없음 (vanilla, opt-in)**. `rocky` 를 권장 소울로 문서에 안내하되 강제 활성화하지 않음.

## 데이터 흐름

```
세션 시작
  └─ SessionStart 훅 (inject-soul.ts)
       ├─ stdin: { cwd, ... }
       ├─ loadConfig({ projectRoot: cwd })  → RockyConfig
       ├─ resolveSoulName(config)           → "rocky" | undefined
       │     └─ undefined → 빈 출력 (vanilla)
       ├─ readSoul("rocky", dirs)           → { body } | null
       │     └─ null → 빈 출력 (fail-open)
       └─ buildSoulContext(soul) → stdout JSON(additionalContext)
             → 세션 컨텍스트에 페르소나 주입

전환: /rocky:soul <name> → rocky.json 의 soul 갱신 → 다음 세션부터 반영
```

## 에러 처리

- 훅은 **항상 exit 0** + 실패 시 빈 출력 — 세션 시작을 막지 않는다.
- config 파싱 오류: `loadConfig` 가 이미 파일별 에러를 격리 반환하므로, 훅은 에러가 있어도 resolve 가능한 만큼만 쓰고 나머지는 stderr 로.
- `soul` 이 존재하지 않는 이름을 가리키면 `readSoul` 이 null → vanilla + stderr 경고.
- `/rocky:soul` 쓰기: 대상 `rocky.json` 이 없으면 생성, 있으면 `soul` 키만 갱신(다른 필드 보존). 커스텀 소울 `new` 는 기존 파일 덮어쓰지 않음.

## 테스트

- `src/core/soul.test.ts` (mkdtemp 격리):
  - `listSouls`: 프리셋+커스텀 머지, 커스텀이 동명 프리셋 덮어씀, source 태깅.
  - `readSoul`: 커스텀 우선, 없는 이름 → null, frontmatter 파싱.
  - `buildSoulContext`: preamble 포함, 본문 래핑.
  - `resolveSoulName`: config 에서 추출, 미설정 → undefined.
- `src/core/rocky-config.test.ts` (기존 파일에 추가): `soul` 필드 검증(정상/이상 이름 reject), `mergeConfigs` 에서 project 가 user 덮어씀, 알 수 없는 top-level 키 가드 유지.
- 훅 엔트리(`inject-soul.ts`)는 얇은 IO 라 별도 유닛 테스트 최소화 — 순수 로직은 core 에서 커버.

## 문서 (change checklist)

- `FEATURES.md`: config 표에 `soul`, 커맨드에 `/rocky:soul`, 훅에 `SessionStart` 자동 주입, 소울 파일 위치 추가.
- `AGENTS.md`: Layout(`souls/`, `commands/soul.md`, `src/core/soul.ts`, `src/hooks/inject-soul.ts`, `hooks/hooks.json` 의 SessionStart) + *Project in one line* 갱신.
- `README.md`: surface 카운트/설명 갱신.
- `plugin.json`: description + keywords(`soul`, `persona`) 갱신.
- `rocky.schema.json`: `soul` 필드 (rocky-config.ts 와 lockstep).

## 미해결/후속 (범위 밖)

- output-style 네이티브 배포와의 통합 여부 — 후속 검토.
- 소울별 MCP 도구 구성 프리셋 — 후속 PR.
- 커스텀 소울 공유/마켓 — 아님.

## 변경 체크리스트 (구현 완료 기준)

1. `bun run check` 통과
2. `bun run typecheck` 통과
3. `bun test` 통과 (신규 soul 테스트 포함)
4. `rocky.json` 모양 변경 → `rocky.schema.json` + `rocky-config.ts` lockstep
5. 표면 변경 → `FEATURES.md` / `AGENTS.md` / `README.md` / `plugin.json` 동기화
6. MCP tool surface 불변 확인 (`src/index.test.ts` 무손상)
