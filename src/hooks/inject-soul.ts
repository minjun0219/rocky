import { loadConfig } from '../core/rocky-config';
import { buildSoulContext, readSoul, resolveSoulName } from '../core/soul';

/**
 * SessionStart hook: 활성 소울(페르소나)을 세션 컨텍스트에 주입한다.
 *   config.soul → 소울 파일(커스텀 우선, 없으면 번들) → additionalContext.
 * 어떤 실패도 세션 시작을 막지 않도록 항상 exit 0, 문제 시 빈 출력(vanilla).
 */

interface SessionStartInput {
  cwd?: string;
}

/**
 * 주입할 컨텍스트 문자열을 SessionStart stdout JSON 으로 만든다. context 가 null 이면
 * (소울 미설정 / 파일 없음) null 을 돌려준다 — caller 는 아무것도 출력하지 않는다.
 */
export function buildInjection(context: string | null): string | null {
  if (!context) {
    return null;
  }
  return JSON.stringify({
    hookSpecificOutput: {
      hookEventName: 'SessionStart',
      additionalContext: context,
    },
  });
}

async function readStdin(): Promise<string> {
  let raw = '';
  for await (const chunk of process.stdin) {
    raw += chunk;
  }
  return raw;
}

/** cwd 로 config 를 읽어 활성 소울 컨텍스트를 만든다. 없으면 null. */
async function resolveContext(cwd: string): Promise<string | null> {
  const { config } = await loadConfig({ projectRoot: cwd });
  const name = resolveSoulName(config);
  if (!name) {
    return null;
  }
  const soul = readSoul(name);
  if (!soul) {
    process.stderr.write(`[rocky soul] configured soul "${name}" not found — skipping\n`);
    return null;
  }
  return buildSoulContext(soul, { callsign: config.callsign });
}

async function run(): Promise<void> {
  const raw = await readStdin();
  let input: SessionStartInput = {};
  try {
    input = JSON.parse(raw) as SessionStartInput;
  } catch {
    // stdin 이 비었거나 JSON 이 아니면 cwd fallback.
  }
  const cwd = input.cwd ?? process.cwd();
  const context = await resolveContext(cwd);
  const out = buildInjection(context);
  if (out) {
    process.stdout.write(out);
  }
}

if (import.meta.main) {
  run()
    .catch(() => {
      // 절대 세션 시작을 막지 않는다 — 모든 오류 삼킴
    })
    .finally(() => process.exit(0));
}
