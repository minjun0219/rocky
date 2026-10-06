/**
 * 가짜 `claude` — 격리 데몬이 부르는 `claude agents --json` 에 정해진 세션 일곱을 낸다.
 *
 * 데몬은 세션 목록을 PATH 의 `claude` 로 받는다. 진짜를 부르면 이 기기에 떠 있는 세션이 화면에 섞여 기기마다
 * 다르고(CI 에는 `claude` 가 없어 "읽지 못했어요" 만 나온다) 에이전트 탭을 고정할 수 없다. 그래서 PATH 맨 앞에
 * `agents --json` 만 아는 sh 를 둔다. background 세션의 작업 요약은 Claude Code 가
 * `<설정 폴더>/jobs/<짧은 id>/state.json` 에 남기므로 데몬에 넘기는 `CLAUDE_CONFIG_DIR` 아래 같은 자리에 심는다.
 *
 * 세션은 보드 판정(cwd 의 경로 세그먼트)만 쓰는 가짜 경로에 있고, 받은편지함 등록이 없어 메시지도 닿지 않는다.
 */
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

/** spec 이 찾는 이름·문구. 묶음은 내 차례 둘 · 실행 중 둘 · 쉬는 중 셋. */
export const AGENTS = {
  /** 답을 기다리는 background(요약 있음, demo) — 피드 "답 기다림" 행의 제목이 이 `needs` 다. */
  blocked: {
    name: 'e2e-wait-migrate',
    needs: '배포 전에 마이그레이션 순서를 확인해 주세요',
  },
  /** 답을 기다리는 background(요약 없음, 보드 없는 레포) — 피드 행 제목이 "답을 기다려요" 로 대신 찬다. */
  blockedBare: { name: 'e2e-wait-bare' },
  /** 도는 background(demo-two) — 탭에 "지금 하는 일" 줄이 붙는다. */
  working: { name: 'e2e-bg-tests', detail: '통합 테스트를 돌리는 중' },
  /** 도는 interactive — demo 레포의 워크트리라 demo 보드로 접힌다. */
  worktree: { name: 'e2e-fix-login' },
} as const;

const MIN = 60_000;

export function installFakeClaude(work: string): { bin: string; configDir: string } {
  const now = Date.now();
  const ago = (minutes: number) => now - minutes * MIN;
  const iso = (ms: number) => new Date(ms).toISOString();
  const sid = (n: number) => `00000000-0000-4000-8000-${String(n).padStart(12, '0')}`;
  // `claude agents --json` 의 행 모양 — 잠든·끝난 background 에는 pid·status 가 없다(Claude Code 2.1.289).
  const sessions = [
    {
      kind: 'background',
      id: 'e2eblk01',
      sessionId: sid(1),
      name: AGENTS.blocked.name,
      cwd: '/e2e/dev/demo',
      state: 'blocked',
      startedAt: ago(40),
    },
    {
      kind: 'background',
      id: 'e2eblk02',
      sessionId: sid(2),
      name: AGENTS.blockedBare.name,
      cwd: '/e2e/dev/other-repo',
      state: 'blocked',
      startedAt: ago(50),
    },
    {
      kind: 'background',
      id: 'e2ewrk01',
      pid: 3_999_001,
      sessionId: sid(3),
      name: AGENTS.working.name,
      cwd: '/e2e/dev/demo-two',
      status: 'busy',
      state: 'working',
      startedAt: ago(5),
    },
    {
      kind: 'interactive',
      pid: 3_999_002,
      sessionId: sid(4),
      name: AGENTS.worktree.name,
      cwd: '/e2e/dev/demo/.claude/worktrees/fix-login',
      status: 'busy',
      startedAt: ago(25),
    },
    {
      kind: 'interactive',
      pid: 3_999_003,
      sessionId: sid(5),
      name: 'e2e-idle-docs',
      cwd: '/e2e/dev/demo-two',
      status: 'idle',
      startedAt: ago(120),
    },
    {
      kind: 'background',
      id: 'e2edone1',
      sessionId: sid(6),
      name: 'e2e-done-lint',
      cwd: '/e2e/dev/demo',
      state: 'done',
      startedAt: ago(180),
    },
    {
      kind: 'interactive',
      pid: 3_999_004,
      sessionId: sid(7),
      name: 'e2e-idle-notes',
      cwd: '/e2e/dev/notes',
      status: 'idle',
      startedAt: ago(1440),
    },
  ];
  const jobs: Record<string, Record<string, string>> = {
    e2eblk01: {
      needs: AGENTS.blocked.needs,
      detail: '마이그레이션 둘이 같은 테이블을 고친다',
      updatedAt: iso(ago(10)),
    },
    e2ewrk01: { detail: AGENTS.working.detail, updatedAt: iso(ago(1)) },
    e2edone1: { detail: '린트 경고를 정리했다', updatedAt: iso(ago(170)) },
  };

  const configDir = join(work, 'claude');
  for (const [id, state] of Object.entries(jobs)) {
    const dir = join(configDir, 'jobs', id);
    mkdirSync(dir, { recursive: true });
    writeFileSync(join(dir, 'state.json'), JSON.stringify(state));
  }
  const bin = join(work, 'bin');
  mkdirSync(bin, { recursive: true });
  writeFileSync(join(bin, 'agents.json'), JSON.stringify(sessions));
  writeFileSync(
    join(bin, 'claude'),
    [
      '#!/bin/sh',
      '# e2e 가짜 claude — `agents --json` 만 안다(e2e/support/claude.ts).',
      'if [ "$1" = agents ] && [ "$2" = --json ]; then',
      '  exec cat "$(dirname "$0")/agents.json"',
      'fi',
      'echo "e2e 가짜 claude: agents --json 만 안다 (받은 인자: $*)" >&2',
      'exit 1',
      '',
    ].join('\n'),
    { mode: 0o755 },
  );
  return { bin, configDir };
}
