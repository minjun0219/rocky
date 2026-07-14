import { existsSync } from 'node:fs';
import { readFile } from 'node:fs/promises';
import { createWorklogFromEnv } from '../core/worklog';
import type { WorklogConfig } from '../core/rocky-config';
import { loadConfig } from '../core/rocky-config';
import { buildTurnContent, extractTurn } from './transcript';

/**
 * Stop hook: 매 응답 종료 시 트랜스크립트에서 이번 턴을 뽑아 `kind:"turn"` 한 줄을
 * append 한다. 결정론적(LLM 0). 어떤 실패도 턴을 막지 않도록 항상 exit 0.
 */

/** env(우선) → config(기본 true). `0/false/off/no` 만 비활성. */
export function shouldCapture(env: NodeJS.ProcessEnv, config: WorklogConfig | undefined): boolean {
  const raw = env.ROCKY_WORKLOG_AUTO_CAPTURE;
  if (typeof raw === 'string' && raw.trim().length > 0) {
    const v = raw.trim().toLowerCase();
    return !(v === '0' || v === 'false' || v === 'off' || v === 'no');
  }
  return config?.autoCapture !== false;
}

interface StopHookInput {
  transcript_path?: string;
  cwd?: string;
}

async function readStdin(): Promise<string> {
  let raw = '';
  for await (const chunk of process.stdin) {
    raw += chunk;
  }
  return raw;
}

async function run(): Promise<void> {
  const raw = await readStdin();
  let input: StopHookInput;
  try {
    input = JSON.parse(raw) as StopHookInput;
  } catch {
    return;
  }
  const projectRoot = input.cwd ?? process.cwd();
  const { config } = await loadConfig({ projectRoot });
  if (!shouldCapture(process.env, config.worklog)) {
    return;
  }
  const path = input.transcript_path;
  if (!path || !existsSync(path)) {
    return;
  }
  const parts = extractTurn(await readFile(path, 'utf8'));
  if (!parts) {
    return;
  }
  const maxChars = config.worklog?.captureMaxChars ?? 800;
  const content = buildTurnContent(parts, maxChars);
  await createWorklogFromEnv(config.worklog).append({ content, kind: 'turn', tags: ['turn'] });
}

if (import.meta.main) {
  run()
    .catch(() => {
      // 절대 턴을 막지 않는다 — 모든 오류 삼킴
    })
    .finally(() => process.exit(0));
}
