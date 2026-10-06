#!/usr/bin/env bun
/**
 * cc-usage 출력을 골든 픽스처로 뜬다 — `rocky statusline --full` 이 같은 바이트를 내는지 대조하는 기준.
 *
 *   bun scripts/cc-usage-capture.ts [cc-usage 바이너리]   # 기본: PATH 의 cc-usage
 *
 * cc-usage 는 아카이브됐다(2026-10-06) — 한 번 뜬 출력이 그대로 기준이다. 새 케이스는 로컬 체크아웃(`CC_USAGE_NOW` 가 있는
 * main)을 빌드해 뜬다. 설치된 `~/.local/bin/cc-usage` 는 그 이전 빌드라 쓰지 않는다.
 * 케이스는 이 파일의 `CASES` 가 정본이고, `crates/rocky-core/tests/fixtures/cc-usage/<이름>/` 에 `case.json` 과
 * `expected.txt` 를 쓴다(있던 폴더는 지우고 새로 쓴다). Rust 쪽 테스트는 Go 없이 그 둘만 읽는다.
 *
 * 격리는 cc-usage `examples/preview.sh` 와 같다 — 임시 HOME · 캐시 · 설정, 없는 keychain 항목·credentials 파일.
 * 시각은 cc-usage 의 테스트 전용 `CC_USAGE_NOW` 로 고정한다(cc-usage#14 이후 바이너리가 필요하다).
 *
 * 설계: docs/design/specs/2026-10-05-cc-usage-mirror-design.md
 */
import { mkdirSync, mkdtempSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';

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
  config?: {
    source?: string;
    alert_percent?: number;
    badges?: Record<string, { emoji?: string; glyph?: string; color?: string }>;
    extra_commands?: { command: string[]; timeout_ms?: number }[];
  };
  env?: Record<string, string>;
  tz?: 'Asia/Seoul' | 'UTC';
  /** 일부러 다르게 둔 곳 — `expected` 에서 바꿔 끼운 뒤 비교한다. */
  allow?: [string, string][];
  /** cc-usage 캐시 `usage.json` 에 심을 내용 — usage API 응답과 크레딧 기준선. Rust 테스트도 같은 값을 읽는다. */
  usage?: Record<string, unknown>;
  /** cc-usage 캐시 `state.json` 에 심을 내용 — stdin 관측값. Rust 테스트도 같은 값을 읽는다. */
  state?: Record<string, unknown>;
  /** 로그인된 계정 이메일 — `~/.claude.json` 에 심는다(배지는 이걸로 고른다). */
  account?: string;
  /** `{{HOME}}/project` 에 git repo 를 만드는 셸 줄들 — `sh -c` 로 차례로 돈다. Rust 테스트가 `GIT_ENV` 로 똑같이 다시 돈다. */
  repo?: string[];
  /** `statusline` 뒤에 붙일 인자(`--source none`). rocky 는 `statusline --full` 뒤에 같은 인자를 붙인다. */
  args?: string[];
  /** 이번 실행의 source 환경 변수 — cc-usage 는 `CC_USAGE_SOURCE`, rocky 는 `ROCKY_STATUSLINE_SOURCE` 로 넣는다. */
  sourceEnv?: string;
};

/** repo 를 만들 때의 git 환경 — 작성자·날짜를 고정해 커밋 해시까지 같게 하고, 사용자 git 설정을 읽지 않는다. */
export const GIT_ENV: Record<string, string> = {
  GIT_AUTHOR_NAME: 'fixture',
  GIT_AUTHOR_EMAIL: 'fixture@example.com',
  GIT_AUTHOR_DATE: '2026-09-16T00:00:00+00:00',
  GIT_COMMITTER_NAME: 'fixture',
  GIT_COMMITTER_EMAIL: 'fixture@example.com',
  GIT_COMMITTER_DATE: '2026-09-16T00:00:00+00:00',
  GIT_CONFIG_NOSYSTEM: '1',
  GIT_CONFIG_GLOBAL: '/dev/null',
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

/** 기준 시각에서 `minutes` 뒤의 RFC3339 — 캐시의 시각 필드. */
const iso = (minutes: number) => new Date(NOW_MS + minutes * 60_000).toISOString();
/** usage API 응답 캐시 — `fetched_at` 은 기준 시각에서 `fetchedAgo` 분 전. */
const fetched = (
  fetchedAgo: number,
  windows: Record<string, unknown>,
  extra?: Record<string, unknown>,
) => ({
  usage: { fetched_at: iso(-fetchedAgo), ...windows, ...(extra ? { extra } : {}) },
});
const cacheWin = (percent: number, resetsIn: number) => ({ percent, resets_at: iso(resetsIn) });
/** 경보 키(`<단계>@<창>@<리셋 시각, 분 단위 UTC>`) — 1 = 임박, 2 = 소진. */
const alertKey = (level: 1 | 2, name: string, resetsAtSec: number) =>
  `${level}@${name}@${new Date(Math.floor(resetsAtSec / 60) * 60_000).toISOString().replace('.000Z', 'Z')}`;
/** 기준 시각에서 `ms` 밀리초 전 — 경보가 오른 시각. */
const msAgo = (ms: number) => new Date(NOW_MS - ms).toISOString();

/** 크레딧 캐시 — 금액은 cent(cc-usage 기본 `credit_divisor: 100`). 방금 받은 응답이라 stale 이 아니다. */
const credits = (
  used: number,
  opts: { limit?: number; enabled?: boolean; baseline?: number; rising?: boolean } = {},
) => ({
  usage: {
    fetched_at: NOW,
    extra: { enabled: opts.enabled ?? true, used_credits: used, monthly_limit: opts.limit },
  },
  ...(opts.baseline === undefined
    ? {}
    : { baseline: { window_key: '5h', credits: opts.baseline, at: NOW } }),
  ...(opts.rising ? { credits_rising_at: NOW } : {}),
});

/** Antigravity(agy) 의 stdin — 1.2.14 실측 꼴에서 식별 정보를 뺐다. 한도는 `rate_limits` 대신 `quota`(남은 비율). */
const agy = (model: string | null, quota: Record<string, unknown>) => ({
  cwd: `${HOME}/project`,
  session_id: 'agy-s',
  product: 'antigravity',
  plan_tier: 'Google AI Pro',
  ...(model === null ? {} : { model: { id: model, display_name: model, effort: 'high' } }),
  workspace: { current_dir: `${HOME}/project`, project_dir: `${HOME}/project` },
  context_window: { used_percentage: 12, context_window_size: 1048576 },
  quota,
});
const bucket = (remaining: unknown, resetsIn: number) => ({
  remaining_fraction: remaining,
  reset_time: iso(resetsIn),
});
/** agy 기본 quota — gemini 5h 86% 남음(오늘 18:00 리셋), 주간 94%, 3p 5h 0%·주간 25%. */
const AGY_QUOTA = {
  'gemini-5h': bucket(0.86, 80),
  'gemini-weekly': bucket(0.94, 3 * 24 * 60),
  '3p-5h': bucket(0, 80),
  '3p-weekly': bucket(0.25, 3 * 24 * 60),
};
/** 같은 머신의 Claude 세션이 남긴 캐시·계정 — agy·none 줄에 새면 안 된다. */
const CLAUDE_LEFTOVERS = {
  account: 'a@example.com',
  config: { badges: { 'a@example.com': { emoji: '🏢' } } },
  state: {
    observed_at: NOW,
    stdin_limits_seen: NOW,
    five_hour: { percent: 30, resets_at: iso(80) },
  },
  usage: credits(1160, { limit: 5000 }),
};

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
  // 폭(COLUMNS)은 크레딧을 상태 줄에 붙일지만 정한다 — 크레딧 금액이 없는 지금 경로에서는 출력이 같아야 한다.
  {
    name: 'columns-narrow',
    why: '좁은 폭 — 줄을 자르지 않는다',
    env: { COLUMNS: '20' },
    stdin: withLimits({ five_hour: win(100), seven_day: win(75, at(3000)) }),
  },
  {
    name: 'columns-wide',
    why: '넓은 폭',
    env: { COLUMNS: '300' },
    stdin: withLimits({ five_hour: win(30), seven_day: win(75, at(3000)) }),
  },
  {
    name: 'columns-invalid',
    why: '읽을 수 없는 폭은 모르는 것',
    env: { COLUMNS: 'wide' },
    stdin: withLimits({ five_hour: win(30) }),
  },
  // 크레딧이 있으면 폭이 줄 구성을 바꾼다 — 붙이면 넘칠 때(COLUMNS - 40 기준) 제 줄로 내린다.
  // 상태 줄 "Opus 5 high · ctx 41% · 5h 70% (↻18:00)" 39칸 + " · " 3칸 + "$38.40 ($50)" 12칸 = 54칸.
  {
    name: 'credits-attached',
    why: '폭을 모르면 크레딧은 상태 줄 끝에 붙는다',
    stdin: withLimits({ five_hour: win(30) }),
    usage: credits(1160, { limit: 5000 }),
  },
  {
    name: 'credits-fits',
    why: '54칸 + 여백 40 = 94 — 딱 들어가면 붙인다',
    env: { COLUMNS: '94' },
    stdin: withLimits({ five_hour: win(30) }),
    usage: credits(1160, { limit: 5000 }),
  },
  {
    name: 'credits-overflows',
    why: '한 칸 모자라면 제 줄로 내린다',
    env: { COLUMNS: '93' },
    stdin: withLimits({ five_hour: win(30) }),
    usage: credits(1160, { limit: 5000 }),
  },
  {
    name: 'credits-wide',
    why: '넓으면 붙인다',
    env: { COLUMNS: '300' },
    stdin: withLimits({ five_hour: win(30) }),
    usage: credits(1160, { limit: 5000 }),
  },
  {
    name: 'credits-no-limit',
    why: '한도를 모르면 쓴 금액임을 밝힌다',
    stdin: withLimits({ five_hour: win(30) }),
    usage: credits(983),
  },
  {
    name: 'credits-spending',
    why: '소진 중 — 이번 window 사용분과 강조, 제 줄',
    env: { COLUMNS: '300' },
    stdin: withLimits({ five_hour: win(100) }, { context_window: { used_percentage: 42 } }),
    usage: credits(1080, { limit: 5000, baseline: 1000, rising: true }),
  },
  {
    name: 'credits-hit-idle',
    why: '한도 소진, 아직 안 깎임 — 다음 prompt 부터',
    stdin: withLimits({ five_hour: win(100) }),
    usage: credits(1160, { limit: 5000 }),
  },
  {
    name: 'credits-disabled',
    why: '크레딧이 꺼진 계정의 소진',
    stdin: withLimits({ five_hour: win(100) }),
    usage: credits(0, { enabled: false }),
  },
  {
    name: 'credits-mostly-used',
    why: '남은 금액 색은 사용률로 — 90% 를 쓰면 빨갛다',
    env: { TERM: 'xterm-256color' },
    stdin: withLimits({ five_hour: win(30) }),
    usage: credits(4500, { limit: 5000 }),
  },
  // 캐시를 심은 경우 — api 모드는 usage 캐시를, stdin 쪽은 6시간 안에 본 관측값(state)을 그린다.
  {
    name: 'cache-api-fresh',
    why: 'api 모드 — 5분 전 응답',
    config: { source: 'api' },
    stdin: base,
    usage: fetched(5, { five_hour: cacheWin(42, 80), seven_day: cacheWin(75, 3000) }),
  },
  {
    name: 'cache-api-stale',
    why: '30분 넘게 묵은 응답은 stale',
    config: { source: 'api' },
    stdin: base,
    usage: fetched(45, { five_hour: cacheWin(42, 80) }),
  },
  {
    name: 'cache-api-error',
    why: '응답 없이 실패만 — 그 이유를 낸다',
    config: { source: 'api' },
    stdin: base,
    usage: { last_error: 'http 500: upstream unavailable', failures: 2, backoff_until: iso(5) },
  },
  {
    name: 'cache-api-long-error',
    why: '긴 에러는 40바이트에서 자른다',
    config: { source: 'api' },
    stdin: base,
    usage: { last_error: 'token not found (keychain: service missing; file: absent)', failures: 1 },
  },
  {
    name: 'cache-api-expired',
    why: '리셋이 지난 캐시 창은 버린다',
    config: { source: 'api' },
    stdin: base,
    usage: fetched(5, { five_hour: cacheWin(90, -1), seven_day: cacheWin(80, 3000) }),
  },
  {
    name: 'cache-auto-recent',
    why: 'auto — 6시간 안에 stdin 한도를 봤으면 그 관측값',
    config: { source: 'auto' },
    stdin: base,
    state: { observed_at: iso(-60), stdin_limits_seen: iso(-60), five_hour: cacheWin(20, 80) },
    usage: fetched(5, { five_hour: cacheWin(70, 80) }),
  },
  {
    name: 'cache-auto-old',
    why: 'auto — stdin 을 본 지 오래면 usage 캐시',
    config: { source: 'auto' },
    stdin: base,
    state: { observed_at: iso(-420), stdin_limits_seen: iso(-420), five_hour: cacheWin(20, 80) },
    usage: fetched(5, { five_hour: cacheWin(70, 80) }),
  },
  {
    name: 'cache-stdin-fallback',
    why: 'stdin 이 한 번 비어도 6시간 안의 관측값',
    stdin: base,
    state: { observed_at: iso(-120), stdin_limits_seen: iso(-120), five_hour: cacheWin(35, 80) },
  },
  {
    name: 'cache-stdin-hit-error',
    why: 'stdin 소진 + 크레딧 조회 실패 — 이유와 조회 중 줄',
    stdin: withLimits({ five_hour: win(100) }),
    usage: { last_error: 'token not found', failures: 1 },
  },
  {
    name: 'cache-stdin-hit-stale',
    why: 'stdin 소진 + 묵은 크레딧 응답',
    stdin: withLimits({ five_hour: win(100) }),
    usage: fetched(50, {}, { enabled: true, used_credits: 1160, monthly_limit: 5000 }),
  },
  // 경보 깜빡임 — 단계가 오른 시각(state.json)부터 6초, 0.5초마다 배지 ↔ 굵은 빨강.
  {
    name: 'alert-burst-off-frame',
    why: '오른 지 0.7초 — 꺼진 프레임(굵은 빨강, 여백 유지)',
    stdin: withLimits({ five_hour: win(100) }),
    state: { alert_key: alertKey(2, '5h', TODAY_1800), alert_at: msAgo(700) },
  },
  {
    name: 'alert-burst-on-frame',
    why: '오른 지 1.2초 — 다시 켜진 프레임',
    stdin: withLimits({ five_hour: win(100) }),
    state: { alert_key: alertKey(2, '5h', TODAY_1800), alert_at: msAgo(1200) },
  },
  {
    name: 'alert-after-burst',
    why: '6초가 지나면 배지로 남는다',
    stdin: withLimits({ five_hour: win(100) }),
    state: { alert_key: alertKey(2, '5h', TODAY_1800), alert_at: msAgo(10_000) },
  },
  {
    name: 'alert-near-7d-off-frame',
    why: '7d 임박의 꺼진 프레임',
    stdin: withLimits({ five_hour: win(30), seven_day: win(95, at(3000)) }),
    state: { alert_key: alertKey(1, '7d', at(3000)), alert_at: msAgo(600) },
  },
  // 계정 배지 — 로그인된 계정(이메일)으로 고른다.
  {
    name: 'badge-emoji',
    why: '이모지 배지',
    account: 'work@example.com',
    config: { badges: { 'work@example.com': { emoji: '🏢' } } },
    stdin: withLimits({ five_hour: win(30) }),
  },
  {
    name: 'badge-glyph-color',
    why: '글리프 + 이름 색',
    account: 'me@example.com',
    config: { badges: { 'me@example.com': { glyph: '◆', color: 'blue' } } },
    stdin: withLimits({ five_hour: win(30) }),
  },
  {
    name: 'badge-256',
    why: '기본 글리프 + 256 색',
    account: 'me@example.com',
    config: { badges: { 'me@example.com': { color: '33' } } },
    stdin: withLimits({ five_hour: win(30) }),
  },
  {
    name: 'badge-unknown-color',
    why: '모르는 색이면 색 없이',
    account: 'me@example.com',
    config: { badges: { 'me@example.com': { color: 'teal' } } },
    stdin: withLimits({ five_hour: win(30) }),
  },
  {
    name: 'badge-other-account',
    why: '목록에 없는 계정은 아무것도 없다',
    account: 'someone@example.com',
    config: { badges: { 'me@example.com': { emoji: '🏢' } } },
    stdin: withLimits({ five_hour: win(30) }),
  },
  {
    name: 'badge-empty-row',
    why: '나머지가 다 비어도 배지는 낸다',
    account: 'me@example.com',
    config: { badges: { 'me@example.com': { emoji: '🏢' } } },
    stdin: '{}',
  },
  {
    name: 'badge-pushes-credit-down',
    why: '배지 폭(2+1)도 폭 판단에 든다 — 배지 없이 딱 맞던 94칸에서 크레딧이 내려간다',
    account: 'me@example.com',
    config: { badges: { 'me@example.com': { emoji: '🏢' } } },
    env: { COLUMNS: '94' },
    stdin: withLimits({ five_hour: win(30) }),
    usage: credits(1160, { limit: 5000 }),
  },
  // extra_commands — 다른 도구의 줄을 아래에 그대로 붙인다.
  {
    name: 'extra-order',
    why: '설정 순서대로, 공백뿐인 줄은 빼고 나머지는 그대로',
    stdin: withLimits({ five_hour: win(30) }),
    config: {
      extra_commands: [
        { command: ['sh', '-c', 'sleep 0.1; echo first'], timeout_ms: 2000 },
        { command: ['printf', 'a\n  \n\n  indented\nb\n'] },
        { command: ['printf', '\u001b[32mgreen\u001b[0m'] },
      ],
    },
  },
  {
    name: 'extra-placeholders',
    why: '{{session_id}} · {{cwd}} 치환',
    stdin: withLimits({ five_hour: win(30) }),
    config: {
      extra_commands: [
        { command: ['echo', 'session={{session_id}}'] },
        { command: ['sh', '-c', 'basename "$1"', 'x', '{{cwd}}'] },
      ],
    },
  },
  {
    name: 'extra-skipped',
    why: '쓰인 placeholder 가 비었거나 command 가 비면 건너뛴다',
    stdin: { ...base, session_id: '' },
    config: {
      extra_commands: [
        { command: ['echo', 'session={{session_id}}'] },
        { command: [] },
        { command: ['echo', 'kept'] },
      ],
    },
  },
  {
    name: 'extra-failures',
    why: '0 이 아닌 종료·실행 파일 없음·출력 없음은 아무것도 붙이지 않는다',
    stdin: withLimits({ five_hour: win(30) }),
    config: {
      extra_commands: [
        { command: ['sh', '-c', 'echo leaked; exit 3'] },
        { command: ['/nonexistent/statusline-tool'] },
        { command: ['true'] },
        { command: ['echo', 'survivor'] },
      ],
    },
  },
  {
    name: 'extra-slow',
    why: '마감을 넘긴 명령, 백그라운드 자식이 stdout 을 붙잡는 명령은 빠진다',
    stdin: withLimits({ five_hour: win(30) }),
    config: {
      extra_commands: [
        { command: ['sh', '-c', 'sleep 1; echo late'], timeout_ms: 100 },
        { command: ['sh', '-c', 'sleep 1 & echo held'] },
        { command: ['sh', '-c', 'sleep 1 >/dev/null & echo held-stderr'] },
        { command: ['echo', 'on time'] },
      ],
    },
  },
  {
    name: 'extra-bytes',
    why: 'UTF-8 이 아닌 출력도 바이트 그대로 붙인다',
    stdin: withLimits({ five_hour: win(30) }),
    config: { extra_commands: [{ command: ['printf', 'a\\377b\\n'] }] },
  },
  // git 세그먼트 — 실제 repo 를 만들어 뜬다.
  {
    name: 'git-clean',
    why: '브랜치만',
    stdin: withLimits({ five_hour: win(30) }),
    repo: ['git init -q -b main', 'printf a > a', 'git add a', 'git commit -q -m one'],
  },
  {
    name: 'git-changes',
    why: 'staged · unstaged · 둘 다 — untracked 는 세지 않는다',
    stdin: withLimits({ five_hour: win(30) }),
    repo: [
      'git init -q -b main',
      'printf a > a && printf b > b && printf c > c',
      'git add a b c',
      'git commit -q -m one',
      'printf a2 > a',
      'printf b2 > b && git add b',
      'printf c2 > c && git add c && printf c3 > c',
      'printf u > untracked',
    ],
  },
  {
    name: 'git-conflict',
    why: '충돌',
    stdin: base,
    repo: [
      'git init -q -b main',
      'printf base > f && git add f && git commit -q -m base',
      'git checkout -q -b other && printf other > f && git commit -q -am other',
      'git checkout -q main && printf main > f && git commit -q -am main',
      'git merge -q other >/dev/null 2>&1 || true',
    ],
  },
  {
    name: 'git-detached',
    why: 'detached — 커밋 7자리',
    stdin: base,
    repo: [
      'git init -q -b main',
      'printf a > a && git add a && git commit -q -m one',
      'printf b > a && git commit -q -am two',
      'git checkout -q HEAD~1',
    ],
  },
  {
    name: 'git-ahead-behind',
    why: '업스트림 대비 ⇡1⇣1',
    stdin: base,
    repo: [
      'git init -q --bare -b main ../remote.git',
      'git init -q -b main',
      'printf a > a && git add a && git commit -q -m one',
      'git remote add origin ../remote.git && git push -q -u origin main 2>/dev/null',
      'git clone -q ../remote.git ../other 2>/dev/null',
      'cd ../other && printf b > b && git add b && git commit -q -m theirs && git push -q 2>/dev/null',
      'printf c > c && git add c && git commit -q -m ours',
      'git fetch -q',
    ],
  },
  {
    name: 'git-not-a-repo',
    why: 'repo 가 아니면 세그먼트만 빠진다',
    stdin: base,
    repo: ['printf a > a'],
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
  {
    name: 'agy-gemini',
    why: 'agy — Gemini 모델은 gemini-* 버킷, Claude 쪽 캐시·배지는 보지 않는다',
    stdin: agy('Gemini 3.8 Flash (High)', AGY_QUOTA),
    ...CLAUDE_LEFTOVERS,
  },
  {
    name: 'agy-3p',
    why: 'agy — Gemini 가 아닌 모델은 3p-* 버킷(5h 소진은 배지로 고정)',
    stdin: agy('Claude Opus 4.6 (Thinking)', AGY_QUOTA),
    ...CLAUDE_LEFTOVERS,
  },
  {
    name: 'agy-source-api',
    why: 'agy 는 source 와 무관하게 quota 로 그린다',
    stdin: agy('GPT-OSS 120B (Medium)', AGY_QUOTA),
    config: { source: 'api' },
  },
  {
    name: 'agy-near',
    why: 'agy 의 임박 경보도 깜빡이지 않고 배지',
    stdin: agy('Gemini 3.8 Flash (High)', {
      'gemini-5h': bucket(0.05, 80),
      'gemini-weekly': bucket(0.2, 3 * 24 * 60),
    }),
  },
  {
    name: 'agy-edge-noise',
    why: '경계의 부동소수 오차는 잘라 쓴다(-1e-9 → 소진)',
    stdin: agy('Gemini 3.8 Flash (High)', { 'gemini-5h': bucket(-1e-9, 80) }),
  },
  {
    name: 'agy-out-of-range',
    why: '단위를 모르는 비율은 창을 비운다',
    stdin: agy('Gemini 3.8 Flash (High)', { 'gemini-5h': bucket(1.5, 80) }),
  },
  {
    name: 'agy-missing-bucket',
    why: '고른 버킷이 없으면 다른 버킷으로 대신하지 않는다',
    stdin: agy('Gemini 3.8 Flash (High)', { '3p-5h': bucket(0.5, 80) }),
  },
  {
    name: 'agy-no-model',
    why: '모델을 모르면 어느 버킷인지 모른다',
    stdin: agy(null, AGY_QUOTA),
  },
  {
    name: 'agy-expired',
    why: '리셋이 지난 창은 버린다',
    stdin: agy('Gemini 3.8 Flash (High)', { 'gemini-5h': bucket(0.1, -10) }),
  },
  {
    name: 'agy-source-none',
    why: 'agy 에 --source none 이면 한도 없이',
    stdin: agy('Gemini 3.8 Flash (High)', AGY_QUOTA),
    args: ['--source', 'none'],
  },
  {
    name: 'claude-with-quota',
    why: 'product 가 antigravity 가 아니면 quota 가 와도 Claude 경로',
    stdin: withLimits(
      { five_hour: win(30) },
      { product: 'claude-code', quota: { 'gemini-5h': bucket(0.1, 80) } },
    ),
  },
  {
    name: 'source-flag-none',
    why: '--source none 은 설정을 이 실행에서만 덮는다',
    stdin: withLimits({ five_hour: win(30) }),
    args: ['--source', 'none'],
    ...CLAUDE_LEFTOVERS,
  },
  {
    name: 'source-env-none',
    why: '환경 변수 none 도 같다',
    stdin: withLimits({ five_hour: win(30) }),
    sourceEnv: 'none',
    ...CLAUDE_LEFTOVERS,
  },
  {
    name: 'source-env-unknown',
    why: '모르는 환경 변수 값은 무시하고 설정을 쓴다',
    stdin: withLimits({ five_hour: win(30) }),
    sourceEnv: 'bogus',
  },
  {
    name: 'source-flag-beats-env',
    why: '플래그가 환경 변수를 이긴다',
    stdin: withLimits({ five_hour: win(30) }),
    sourceEnv: 'none',
    args: ['--source', 'stdin'],
  },
  {
    name: 'source-flag-api',
    why: '--source api 면 stdin 대신 usage 캐시',
    stdin: withLimits({ five_hour: win(30) }),
    args: ['--source', 'api'],
    usage: fetched(1, { five_hour: cacheWin(55, 80) }),
  },
  {
    name: 'source-flag-unknown',
    why: '모르는 플래그 값은 무엇이 틀렸는지 한 줄',
    stdin: withLimits({ five_hour: win(30) }),
    args: ['--source', 'bogus'],
    allow: EMPTY_ROW,
  },
];

function capture(bin: string, c: Case, dir: string): { stdin: string; stdout: Buffer } {
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
  if (c.repo) {
    const project = join(home, 'project');
    mkdirSync(project, { recursive: true });
    for (const line of c.repo) {
      const step = Bun.spawnSync(['sh', '-c', line], {
        cwd: project,
        env: { PATH: process.env.PATH ?? '/usr/bin:/bin', HOME: home, ...GIT_ENV },
      });
      if (step.exitCode !== 0) {
        throw new Error(`${c.name}: repo 준비 실패 — ${line}: ${step.stderr.toString()}`);
      }
    }
  }
  if (c.account) {
    writeFileSync(
      join(home, '.claude.json'),
      JSON.stringify({ oauthAccount: { emailAddress: c.account } }),
    );
  }
  mkdirSync(join(dir, 'cache', 'cc-usage'), { recursive: true });
  if (c.usage) {
    writeFileSync(join(dir, 'cache', 'cc-usage', 'usage.json'), JSON.stringify(c.usage));
  }
  if (c.state) {
    writeFileSync(join(dir, 'cache', 'cc-usage', 'state.json'), JSON.stringify(c.state));
  }
  const stdin = typeof c.stdin === 'string' ? c.stdin : JSON.stringify(c.stdin);
  const proc = Bun.spawnSync([bin, 'statusline', ...(c.args ?? [])], {
    stdin: new TextEncoder().encode(stdin.replaceAll(HOME, home)),
    env: {
      PATH: process.env.PATH ?? '/usr/bin:/bin',
      HOME: home,
      XDG_CACHE_HOME: join(dir, 'cache'),
      CC_USAGE_CONFIG: config,
      CC_USAGE_NOW: NOW,
      TZ: c.tz ?? 'Asia/Seoul',
      ...(c.sourceEnv === undefined ? {} : { CC_USAGE_SOURCE: c.sourceEnv }),
      ...c.env,
    },
  });
  if (proc.exitCode !== 0) {
    throw new Error(`${c.name}: cc-usage 종료 코드 ${proc.exitCode} — ${proc.stderr.toString()}`);
  }
  // 바이트 그대로 — 다른 도구가 UTF-8 이 아닌 출력을 낼 수 있다.
  return { stdin, stdout: proc.stdout };
}

if (import.meta.main) {
  const bin = process.argv[2] ?? 'cc-usage';
  const root = join(import.meta.dir, '..', 'crates', 'rocky-core', 'tests', 'fixtures', 'cc-usage');
  // 전부 뜬 다음에만 바꿔 끼운다 — 중간에 실패하면 있던 골든이 그대로 남는다. 같은 폴더 안에 떠야 rename 이
  // 파일 시스템을 건너지 않는다.
  mkdirSync(dirname(root), { recursive: true });
  const staging = mkdtempSync(join(dirname(root), '.cc-usage-staging-'));
  try {
    for (const c of CASES) {
      const dir = mkdtempSync(join(tmpdir(), 'cc-usage-capture-'));
      try {
        const { stdin, stdout } = capture(bin, c, dir);
        const out = join(staging, c.name);
        mkdirSync(out, { recursive: true });
        const meta = {
          why: c.why,
          now: NOW,
          tz: c.tz ?? 'Asia/Seoul',
          env: c.env ?? {},
          config: { source: 'stdin', ...c.config },
          stdin,
          allow: c.allow ?? [],
          usage: c.usage ?? null,
          state: c.state ?? null,
          account: c.account ?? null,
          repo: c.repo ?? [],
          gitEnv: c.repo ? GIT_ENV : {},
          args: c.args ?? [],
          sourceEnv: c.sourceEnv ?? null,
        };
        writeFileSync(join(out, 'case.json'), `${JSON.stringify(meta, null, 2)}\n`);
        writeFileSync(join(out, 'expected.txt'), stdout);
      } finally {
        rmSync(dir, { recursive: true, force: true });
      }
    }
  } catch (e) {
    rmSync(staging, { recursive: true, force: true });
    throw e;
  }
  rmSync(root, { recursive: true, force: true });
  renameSync(staging, root);
  console.log(`${CASES.length}건 → ${root}`);
}
