#!/usr/bin/env bun
/**
 * cc-usage 출력을 골든 픽스처로 뜬다 — `rocky statusline --full` 이 같은 바이트를 내는지 대조하는 기준.
 *
 *   bun scripts/cc-usage-capture.ts [cc-usage 바이너리]   # 기본: PATH 의 cc-usage
 *
 * cc-usage 는 기능 동결 상태라 한 번 뜬 출력이 그대로 기준으로 남는다. 버그 수정으로 출력이 바뀌면 다시 뜬다.
 * 케이스는 이 파일의 `CASES` 가 정본이고, `crates/rocky-core/tests/fixtures/cc-usage/<이름>/` 에 `case.json` 과
 * `expected.txt` 를 쓴다(있던 폴더는 지우고 새로 쓴다). Rust 쪽 테스트는 Go 없이 그 둘만 읽는다.
 *
 * 격리는 cc-usage `examples/preview.sh` 와 같다 — 임시 HOME · 캐시 · 설정, 없는 keychain 항목·credentials 파일.
 * 시각은 cc-usage 의 테스트 전용 `CC_USAGE_NOW` 로 고정한다(cc-usage#14 이후 바이너리가 필요하다).
 *
 * 설계: docs/design/specs/2026-10-05-cc-usage-mirror-design.md
 */
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

/** 모든 케이스의 기준 시각 — 2026-09-16 16:40 KST. */
const NOW = '2026-09-16T16:40:00+09:00';
const NOW_MS = Date.parse(NOW);
/** 기준 시각에서 `minutes` 뒤의 epoch 초. */
const at = (minutes: number) => Math.floor(NOW_MS / 1000) + minutes * 60;
/** 오늘 18:00 KST — 기준 시각에서 80분 뒤. */
const TODAY_1800 = at(80);

/** stdin 의 홈 디렉터리 자리. 캡처는 임시 HOME 으로, Rust 테스트는 고정 경로로 바꾼다. */
const HOME = '{{HOME}}';

export type Case = {
  name: string;
  why: string;
  /** stdin 원문 — 객체면 JSON 으로 쓴다. */
  stdin: string | Record<string, unknown>;
  config?: { source?: string; alert_percent?: number };
  env?: Record<string, string>;
  tz?: 'Asia/Seoul' | 'UTC';
  /** 일부러 다르게 둔 곳 — `expected` 에서 바꿔 끼운 뒤 비교한다. */
  allow?: [string, string][];
};

const base = {
  session_id: 's',
  model: { display_name: 'Opus 5' },
  effort: { level: 'high' },
  context_window: { used_percentage: 41 },
  workspace: { current_dir: `${HOME}/project` },
};
const win = (used: number, resets_at: unknown = TODAY_1800) => ({
  used_percentage: used,
  resets_at,
});
const withLimits = (rate_limits: Record<string, unknown>, extra: Record<string, unknown> = {}) => ({
  ...base,
  ...extra,
  rate_limits,
});
const EMPTY_ROW: [string, string][] = [['[cc-usage]', '[rocky]']];

export const CASES: Case[] = [
  { name: 'basic', why: '5h 만, 오늘 안의 리셋', stdin: withLimits({ five_hour: win(30) }) },
  {
    name: 'seven-day-hidden',
    why: '7d 사용률 70% 아래는 숨긴다',
    stdin: withLimits({ five_hour: win(30), seven_day: win(40, at(3000)) }),
  },
  {
    name: 'seven-day-shown',
    why: '7d 70% 이상은 날이 바뀐 리셋을 남은 시간으로',
    stdin: withLimits({ five_hour: win(30), seven_day: win(75, at(52 * 60)) }),
  },
  { name: 'near-5h', why: '임박(90%) 배지', stdin: withLimits({ five_hour: win(92) }) },
  {
    name: 'over-5h',
    why: '소진 배지 + 크레딧 조회 중 줄',
    stdin: withLimits({ five_hour: win(100), seven_day: win(50, at(3000)) }),
  },
  {
    name: 'over-7d-wins',
    why: '둘 다 소진이면 7d 가 배지',
    stdin: withLimits({ five_hour: win(100), seven_day: win(100, at(3000)) }),
  },
  {
    name: 'near-7d-below-show',
    why: '경보를 올린 7d 는 70% 아래여도 보인다',
    config: { alert_percent: 60 },
    stdin: withLimits({ five_hour: win(30), seven_day: win(65, at(3000)) }),
  },
  {
    name: 'alert-off',
    why: 'alert_percent 0 은 임박 경고를 끈다',
    config: { alert_percent: 0 },
    stdin: withLimits({ five_hour: win(95) }),
  },
  {
    name: 'alert-out-of-range',
    why: '범위 밖 alert_percent 는 끈 것',
    config: { alert_percent: 150 },
    stdin: withLimits({ five_hour: win(95) }),
  },
  { name: 'no-rate-limits', why: 'stdin 에 한도가 없다', stdin: base },
  { name: 'empty-stdin', why: '빈 입력 — 최소 한 줄', stdin: '', allow: EMPTY_ROW },
  { name: 'invalid-json', why: 'JSON 이 아니다', stdin: '{"model":', allow: EMPTY_ROW },
  {
    name: 'source-none',
    why: '한도를 다루지 않는다',
    config: { source: 'none' },
    stdin: withLimits({ five_hour: win(100) }),
  },
  {
    name: 'source-api',
    why: 'api 는 stdin 을 보지 않는다 — 조회 대기',
    config: { source: 'api' },
    stdin: withLimits({ five_hour: win(30) }),
  },
  {
    name: 'source-auto-stdin',
    why: 'auto 인데 stdin 에 한도가 있다',
    config: { source: 'auto' },
    stdin: withLimits({ five_hour: win(30) }),
  },
  {
    name: 'source-auto-empty',
    why: 'auto 인데 stdin 에 한도가 없다 — 조회 대기',
    config: { source: 'auto' },
    stdin: base,
  },
  {
    name: 'no-color',
    why: 'NO_COLOR',
    env: { NO_COLOR: '1' },
    stdin: withLimits({ five_hour: win(92), seven_day: win(75, at(3000)) }),
  },
  {
    name: 'term-256',
    why: '256색 큐브',
    env: { TERM: 'xterm-256color' },
    stdin: withLimits(
      { five_hour: win(71), seven_day: win(89, at(3000)) },
      { context_window: { used_percentage: 85 } },
    ),
  },
  {
    name: 'truecolor',
    why: '24bit 그라데이션',
    env: { COLORTERM: 'truecolor' },
    stdin: withLimits(
      { five_hour: win(10), seven_day: win(80, at(3000)) },
      { context_window: { used_percentage: 96 } },
    ),
  },
  {
    name: 'expired-window',
    why: '리셋이 지난 창은 버린다',
    stdin: withLimits({ five_hour: win(80, at(-5)) }),
  },
  {
    name: 'resets-formats',
    why: '밀리초 · RFC3339 · 숫자 문자열',
    stdin: withLimits({
      five_hour: win(30, TODAY_1800 * 1000),
      seven_day: win(75, '2026-09-18T00:00:00Z'),
    }),
  },
  {
    name: 'resets-numeric-string',
    why: '숫자 문자열 리셋',
    stdin: withLimits({ five_hour: win(30, String(TODAY_1800)) }),
  },
  {
    name: 'epoch-leak',
    why: '말이 안 되는 사용률은 버리고 100 넘는 값은 100',
    stdin: withLimits({ five_hour: win(1.7e9), seven_day: win(150, at(3000)) }),
  },
  {
    name: 'half-rounding',
    why: '반올림 — 70.5 · 40.5',
    stdin: withLimits({ five_hour: win(29.5) }, { context_window: { used_percentage: 40.5 } }),
  },
  {
    name: 'reset-tomorrow',
    why: '24시간 안이어도 날이 바뀌면 남은 시간',
    stdin: withLimits({ five_hour: win(30, at(500)) }),
  },
  {
    name: 'reset-under-minute',
    why: '1분 안의 리셋',
    stdin: withLimits({ five_hour: win(30, at(0.5)) }),
  },
  {
    name: 'tz-utc',
    why: '리셋 시각은 로컬 시간대로',
    tz: 'UTC',
    stdin: withLimits({ five_hour: win(30) }),
  },
  {
    name: 'dir-outside-home',
    why: '홈 밖 경로는 그대로',
    stdin: withLimits({ five_hour: win(30) }, { workspace: { current_dir: '/fixture/elsewhere' } }),
  },
  { name: 'dir-is-home', why: '홈 자체는 ~', stdin: { ...base, workspace: { current_dir: HOME } } },
  {
    name: 'cwd-fallback',
    why: 'workspace 가 없으면 cwd',
    stdin: { session_id: 's', model: { display_name: 'Opus 5' }, cwd: `${HOME}/other` },
  },
  { name: 'model-only', why: 'effort 없음', stdin: { model: { display_name: 'Sonnet 5' } } },
  { name: 'effort-only', why: '모델 없음', stdin: { effort: { level: 'max' } } },
  {
    name: 'type-mismatch',
    why: '타입이 어긋난 필드만 빈다',
    stdin: {
      model: 'Opus 5',
      effort: { level: 'low' },
      context_window: { used_percentage: '41' },
      rate_limits: { five_hour: { used_percentage: '30' } },
    },
  },
];

function capture(bin: string, c: Case, dir: string): { stdin: string; stdout: string } {
  const home = join(dir, 'home');
  mkdirSync(home, { recursive: true });
  const config = join(dir, 'config.json');
  writeFileSync(
    config,
    JSON.stringify({
      source: 'stdin',
      ...c.config,
      keychain_service: 'cc-usage-capture-absent',
      credentials_file: join(dir, 'absent.json'),
    }),
  );
  const stdin = typeof c.stdin === 'string' ? c.stdin : JSON.stringify(c.stdin);
  const proc = Bun.spawnSync([bin, 'statusline'], {
    stdin: new TextEncoder().encode(stdin.replaceAll(HOME, home)),
    env: {
      PATH: process.env.PATH ?? '/usr/bin:/bin',
      HOME: home,
      XDG_CACHE_HOME: join(dir, 'cache'),
      CC_USAGE_CONFIG: config,
      CC_USAGE_NOW: NOW,
      TZ: c.tz ?? 'Asia/Seoul',
      ...c.env,
    },
  });
  if (proc.exitCode !== 0) {
    throw new Error(`${c.name}: cc-usage 종료 코드 ${proc.exitCode} — ${proc.stderr.toString()}`);
  }
  return { stdin, stdout: proc.stdout.toString() };
}

if (import.meta.main) {
  const bin = process.argv[2] ?? 'cc-usage';
  const root = join(import.meta.dir, '..', 'crates', 'rocky-core', 'tests', 'fixtures', 'cc-usage');
  rmSync(root, { recursive: true, force: true });
  for (const c of CASES) {
    const dir = mkdtempSync(join(tmpdir(), 'cc-usage-capture-'));
    try {
      const { stdin, stdout } = capture(bin, c, dir);
      const out = join(root, c.name);
      mkdirSync(out, { recursive: true });
      const meta = {
        why: c.why,
        now: NOW,
        tz: c.tz ?? 'Asia/Seoul',
        env: c.env ?? {},
        config: { source: 'stdin', ...c.config },
        stdin,
        allow: c.allow ?? [],
      };
      writeFileSync(join(out, 'case.json'), `${JSON.stringify(meta, null, 2)}\n`);
      writeFileSync(join(out, 'expected.txt'), stdout);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  }
  console.log(`${CASES.length}건 → ${root}`);
}
