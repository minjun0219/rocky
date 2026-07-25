import { appendFileSync } from 'node:fs';
import { createJobStoreFromEnv, isTerminal } from '../core/opencode-jobs';
import { killJobProcess } from '../core/opencode-runner';
import { loadConfig } from '../core/rocky-config';

/**
 * SessionStart / SessionEnd hook — `/rocky:opencode` 백그라운드 잡의 세션 배선.
 *
 * **SessionStart**: `CLAUDE_ENV_FILE` 에 `ROCKY_SESSION_ID` 를 export 로 append 한다.
 * 이후 슬래시 커맨드의 모든 Bash 호출이 이 env 를 물려받아, 만들어지는 잡에 세션 id 가 박히고
 * `status` / `cancel` 이 **다른 세션의 잡을 건드리지 않는다.** matcher 를 붙이지 않는 이유가
 * 여기 있다 — `resume` 에서 주입이 빠지면 재개된 세션의 잡이 전부 필터에서 새어 나간다.
 *
 * **SessionEnd**: 이 세션이 띄운 진행 중 잡의 프로세스 그룹을 끊는다. 세션이 사라졌는데
 * detached 워커만 남아 계속 도는 고아 상태를 막는다. 다만 codex 와 달리 **잡 기록은 지우지
 * 않는다** — rocky 는 사후에 무엇이 돌았는지 되짚을 수 있어야 하고, 오래된 기록은 저장소의
 * `maxJobs` prune 이 알아서 정리한다.
 *
 * 어떤 실패도 세션을 막지 않도록 항상 exit 0 으로 끝난다 (fail-open).
 */

interface SessionHookInput {
  session_id?: string;
  cwd?: string;
  hook_event_name?: string;
}

/** POSIX sh 작은따옴표 이스케이프 — 값 안의 `'` 를 `'\''` 로 끊어 붙인다. */
export function shellQuote(value: string): string {
  return `'${value.replace(/'/g, `'\\''`)}'`;
}

/**
 * `CLAUDE_ENV_FILE` 에 쓸 export 구문. 세션 id 가 없으면 빈 문자열 —
 * 빈 값을 export 하면 필터가 "세션 없음" 과 구분되지 않아 오히려 위험하다.
 */
export function buildEnvExports(sessionId: string | undefined): string {
  const trimmed = sessionId?.trim();
  if (!trimmed) {
    return '';
  }
  return `export ROCKY_SESSION_ID=${shellQuote(trimmed)}\n`;
}

async function readStdin(): Promise<string> {
  let raw = '';
  for await (const chunk of process.stdin) {
    raw += chunk;
  }
  return raw;
}

/** SessionStart — 세션 id 를 후속 Bash 호출에 전파한다. */
function handleStart(input: SessionHookInput): void {
  const envFile = process.env.CLAUDE_ENV_FILE;
  const exports = buildEnvExports(input.session_id);
  if (!envFile || exports.length === 0) {
    return;
  }
  appendFileSync(envFile, exports, 'utf8');
}

/** SessionEnd — 이 세션의 진행 중 잡을 끊는다 (기록은 남긴다). */
async function handleEnd(input: SessionHookInput): Promise<void> {
  const sessionId = input.session_id?.trim();
  if (!sessionId) {
    return;
  }
  const cwd = input.cwd ?? process.cwd();
  const { config } = await loadConfig({ projectRoot: cwd });
  const store = createJobStoreFromEnv(config.opencode, process.env, cwd);
  for (const job of store.list()) {
    if (job.sessionId !== sessionId || isTerminal(job.status)) {
      continue;
    }
    // 잡 하나의 실패가 루프를 끊으면 같은 세션의 다른 워커가 고아로 남는다 —
    // 고아를 막으려고 만든 훅이 정작 고아를 만드는 셈이라, 개별 실패를 격리한다.
    try {
      // payload 가 없는 잡은 `list()` 가 인덱스로 복원한 것이라 pid 를 모른다. 끊을 수단이
      // 없으니 건너뛴다 — 여기서 update 를 시도하면 예외로 나머지 정리까지 막힌다.
      if (!store.get(job.id)) {
        continue;
      }
      const outcome = killJobProcess(job);
      store.appendLog(job.id, `세션 종료로 정리: ${outcome.detail}`);
      store.update(job.id, {
        status: 'cancelled',
        phase: 'done',
        errorMessage: `세션 종료로 중단 (${outcome.detail})`,
        completedAt: new Date().toISOString(),
      });
    } catch {
      // 다음 잡 정리를 계속한다.
    }
  }
}

async function run(): Promise<void> {
  let input: SessionHookInput;
  try {
    input = JSON.parse(await readStdin()) as SessionHookInput;
  } catch {
    return;
  }
  const event = process.argv[2] ?? input.hook_event_name;
  if (event === 'SessionStart') {
    handleStart(input);
    return;
  }
  if (event === 'SessionEnd') {
    await handleEnd(input);
  }
}

if (import.meta.main) {
  try {
    await run();
  } catch {
    // fail-open — 훅이 세션을 막으면 안 된다.
  }
  process.exit(0);
}
