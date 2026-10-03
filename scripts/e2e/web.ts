#!/usr/bin/env bun
/**
 * 웹 UI E2E 시나리오 — `bun run e2e`.
 *
 * 전용 임시 폴더에 설정·DB 를 만들고 `target/debug/rockyd` 를 따로 띄운 뒤, REST 로 가짜 픽스처를 심고
 * Playwright 로 폰·cmux 옆 창·데스크톱 세 화면에서 피드 → 할 일 → 노트 → 작업로그 → GitHub → 메뉴 →
 * 퍼머링크까지 한 바퀴 돈다. 실제 보드·작업로그·사용 로그·GitHub 계정에는 닿지 않는다 — HOME 까지
 * 임시 폴더로 돌리고 환경 변수는 필요한 것만 넘긴다.
 *
 * 결과: 단계 실패·JS 에러가 있으면 exit 1. 화면 점검 발견(가로 넘침·작은 글자·작은 터치 타깃)은
 * 경고로만 내고 exit 0 — `--strict` 면 발견도 실패다.
 *
 * 옵션: `--out <dir>` 스크린샷 폴더(기본: 새 임시 폴더, 지우지 않는다) · `--strict` · `--no-build`
 * (UI·데몬 빌드를 건너뛴다) · `--headed`.
 *
 * 브라우저: playwright-core 가 받아 둔 Chromium(`bunx playwright-core install chromium`)을 먼저 쓰고,
 * 없으면 설치된 Google Chrome(`channel: 'chrome'`)으로 넘어간다. 스크립트가 직접 내려받지는 않는다.
 */
import { closeSync, mkdirSync, mkdtempSync, openSync, rmSync, writeFileSync } from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import { join } from 'node:path';
import {
  type Browser,
  type BrowserContext,
  chromium,
  type Locator,
  type Page,
} from 'playwright-core';

const root = join(import.meta.dir, '..', '..');

export type Env = {
  name: string;
  viewport: { width: number; height: number };
  isMobile?: boolean;
  hasTouch?: boolean;
  deviceScaleFactor: number;
  scheme: 'light' | 'dark';
};

export const ENVS: Env[] = [
  {
    name: 'phone',
    viewport: { width: 390, height: 844 },
    isMobile: true,
    hasTouch: true,
    deviceScaleFactor: 2,
    scheme: 'light',
  },
  { name: 'cmux', viewport: { width: 360, height: 860 }, deviceScaleFactor: 2, scheme: 'dark' },
  {
    name: 'desktop',
    viewport: { width: 1280, height: 860 },
    deviceScaleFactor: 1,
    scheme: 'light',
  },
];

/** 기능 실패 — 이것이 하나라도 있으면 exit 1. 나머지 종류는 화면 점검 발견이다. */
export const FAILURE_KINDS: ReadonlySet<string> = new Set(['실패', 'JS 에러', '콘솔 에러']);

export type Finding = { env: string; step: string; kind: string; detail: string };

export type Options = { out?: string; strict: boolean; build: boolean; headed: boolean };

export function parseArgs(argv: string[]): Options {
  const opts: Options = { strict: false, build: true, headed: false };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === '--strict') {
      opts.strict = true;
    } else if (arg === '--no-build') {
      opts.build = false;
    } else if (arg === '--headed') {
      opts.headed = true;
    } else if (arg === '--out') {
      const value = argv[++i];
      if (!value) {
        throw new Error('--out 에는 폴더 경로가 필요하다');
      }
      opts.out = value;
    } else {
      throw new Error(`알 수 없는 옵션: ${arg}`);
    }
  }
  return opts;
}

/** 같은 (환경·단계·종류·내용)은 한 번만. */
export function dedupe(findings: Finding[]): Finding[] {
  const seen = new Set<string>();
  return findings.filter((f) => {
    const key = `${f.env}\u0000${f.step}\u0000${f.kind}\u0000${f.detail}`;
    if (seen.has(key)) {
      return false;
    }
    seen.add(key);
    return true;
  });
}

/** 기능 실패가 있으면 1, 발견만 있으면 `strict` 일 때만 1. */
export function exitCodeOf(findings: Finding[], strict: boolean): number {
  if (findings.some((f) => FAILURE_KINDS.has(f.kind))) {
    return 1;
  }
  return strict && findings.length > 0 ? 1 : 0;
}

// ── 데몬 ──────────────────────────────────────────────────────────────

function run(cmd: string[]): void {
  const result = Bun.spawnSync(cmd, { cwd: root, stdout: 'inherit', stderr: 'inherit' });
  if (result.exitCode !== 0) {
    throw new Error(`${cmd.join(' ')} 실패 (exit ${result.exitCode})`);
  }
}

function freePort(): number {
  const server = Bun.serve({ port: 0, hostname: '127.0.0.1', fetch: () => new Response() });
  const port = server.port;
  server.stop(true);
  if (!port) {
    throw new Error('빈 포트를 얻지 못했다');
  }
  return port;
}

type Daemon = { base: string; proc: Bun.Subprocess; log: string };

async function startDaemon(work: string): Promise<Daemon> {
  const port = freePort();
  const config = join(work, 'rocky.json');
  // 전부 임시 폴더 안 — 실제 보드 DB·작업로그·사용 로그를 읽지도 쓰지도 않는다.
  writeFileSync(
    config,
    JSON.stringify(
      {
        todo: { port, dir: join(work, 'todo'), expose: 'off' },
        pr: { enabled: false },
        worklog: { dir: join(work, 'worklog') },
        usage: { enabled: false },
      },
      null,
      2,
    ),
  );
  const log = join(work, 'rockyd.log');
  const fd = openSync(log, 'w');
  // 환경은 물려받지 않는다: GH_TOKEN·ROCKY_* 같은 값이 새면 실제 계정·설정에 닿는다. HOME 도 임시 폴더라
  // `~/.config` 기본값이나 gh 설정을 읽지 않는다.
  const proc = Bun.spawn([join(root, 'target', 'debug', 'rockyd')], {
    cwd: root, // 데몬은 cwd 의 dist/ 도 찾는다 — 그래도 아래 ROCKY_TODO_UI_DIST 로 못 박는다.
    env: {
      PATH: process.env.PATH ?? '/usr/bin:/bin',
      HOME: work,
      ROCKY_CONFIG: config,
      ROCKY_TODO_UI_DIST: join(root, 'dist'),
      ROCKY_USAGE: '0',
    },
    stdout: fd,
    stderr: fd,
  });
  closeSync(fd); // 자식이 복제해 갖고 있다
  const base = `http://127.0.0.1:${port}`;
  const deadline = Date.now() + 20_000;
  while (Date.now() < deadline) {
    if (proc.exitCode !== null) {
      throw new Error(`rockyd 가 바로 끝났다 (exit ${proc.exitCode}) — 로그: ${log}`);
    }
    try {
      const res = await fetch(`${base}/api/health`);
      const body = (await res.json()) as { name?: string };
      if (res.ok && body.name === 'rocky') {
        return { base, proc, log };
      }
    } catch {
      // 아직 안 떴다
    }
    await Bun.sleep(200);
  }
  await stopDaemon({ base, proc, log });
  throw new Error(`rockyd health 가 20초 안에 오지 않았다 (${base}) — 로그: ${log}`);
}

/** 우리가 띄운 pid 만 내린다 — 패턴으로 찾아 죽이지 않는다(다른 rockyd 가 같이 돌 수 있다). */
async function stopDaemon(daemon: Daemon): Promise<void> {
  if (daemon.proc.exitCode !== null) {
    return;
  }
  daemon.proc.kill('SIGTERM');
  const exited = await Promise.race([
    daemon.proc.exited.then(() => true),
    Bun.sleep(5000).then(() => false),
  ]);
  if (!exited) {
    daemon.proc.kill('SIGKILL');
    await daemon.proc.exited;
  }
}

// ── 픽스처 ────────────────────────────────────────────────────────────

type Seeded = { permalinkNumber: number };

async function seed(base: string): Promise<Seeded> {
  const call = async <T>(
    method: string,
    path: string,
    body: unknown,
    actor = 'human',
  ): Promise<T> => {
    const res = await fetch(`${base}${path}`, {
      method,
      headers: { 'content-type': 'application/json', 'x-rocky-actor': actor },
      body: JSON.stringify(body),
    });
    if (!res.ok) {
      throw new Error(`픽스처 ${method} ${path} → ${res.status} ${await res.text()}`);
    }
    return (await res.json()) as T;
  };
  type Todo = { id: string; ref: string };

  await call('POST', '/api/boards', { key: 'demo', title: 'Demo' });
  await call('POST', '/api/boards', { key: 'demo-two', title: 'Demo Two' });
  await call('POST', '/api/sections', { board: 'demo', title: '이번 주' });

  const first = await call<Todo>('POST', '/api/todos', {
    board: 'demo',
    title: '로그인 화면 문구 다듬기',
    description: '버튼 문구와 **오류 메시지**를 맞춘다.\n\n- 빈 입력\n- 잘못된 비밀번호',
    priority: 'p1',
    labels: ['ui', 'copy'],
    links: [{ url: 'https://example.com/spec/login', title: '로그인 스펙' }],
    section: '이번 주',
  });
  await call('POST', '/api/todos', {
    board: 'demo',
    title: '하위: 오류 메시지 목록',
    parentId: first.id,
  });
  await call('POST', '/api/todos', {
    board: 'demo',
    title: '하위: 문구 검토 요청',
    parentId: first.id,
  });
  const others = [
    await call<Todo>('POST', '/api/todos', {
      board: 'demo',
      title: '설정 화면 정리',
      priority: 'p2',
      labels: ['ui'],
    }),
    await call<Todo>('POST', '/api/todos', {
      board: 'demo',
      title: '의존성 버전 점검',
      priority: 'p3',
    }),
    await call<Todo>('POST', '/api/todos', {
      board: 'demo-two',
      title: '두 번째 보드의 할 일',
      priority: 'p2',
    }),
    await call<Todo>('POST', '/api/todos', {
      board: 'demo-two',
      title: '문서 링크 모으기',
      labels: ['docs'],
    }),
  ];
  // 읽지 않은 댓글 — 새 브라우저는 전부 안 읽음이라 4건 이상이면 "N건 더 보기" 요약 줄이 생긴다.
  for (const [i, todo] of [first, ...others].entries()) {
    await call('POST', `/api/todos/${todo.id}/comments`, { body: `진행 메모 ${i + 1}` }, 'claude');
  }
  const done = await call<Todo>('POST', '/api/todos', {
    board: 'demo',
    title: '끝난 일 하나',
    priority: 'p3',
  });
  await call('POST', `/api/todos/${done.id}/status`, { action: 'done' });

  await call('POST', '/api/notes', {
    board: 'demo',
    title: '회의 메모',
    content: '## 결정\n\n- 다음 주 배포',
  });

  const n = Number(first.ref.slice(first.ref.lastIndexOf('-') + 1));
  if (!Number.isInteger(n)) {
    throw new Error(`todo ref 에서 번호를 못 읽었다: ${first.ref}`);
  }
  return { permalinkNumber: n };
}

// ── 화면 점검 ─────────────────────────────────────────────────────────

type AuditResult = { hScroll: number; offRight: string[]; small: string[]; tiny: string[] };

/** 가로 넘침, 화면 밖으로 나간 요소(overflow 조상 아래는 제외), 12px 미만 글자, (터치) 24px 미만 타깃. */
function auditInPage(touch: boolean): AuditResult {
  const vw = window.innerWidth;
  const out: AuditResult = {
    hScroll: document.documentElement.scrollWidth - vw,
    offRight: [],
    small: [],
    tiny: [],
  };
  const visible = (el: Element) => {
    const cs = getComputedStyle(el);
    if (cs.visibility === 'hidden' || cs.display === 'none' || Number(cs.opacity) === 0) {
      return false;
    }
    const b = el.getBoundingClientRect();
    return b.width > 0 && b.height > 0;
  };
  const clippedByScroller = (el: Element) => {
    for (let a = el.parentElement; a; a = a.parentElement) {
      const o = getComputedStyle(a).overflowX;
      if (o === 'auto' || o === 'scroll' || o === 'hidden' || o === 'clip') {
        return true;
      }
    }
    return false;
  };
  const label = (el: Element) =>
    String(el.getAttribute('aria-label') || el.textContent || el.className || el.tagName)
      .trim()
      .slice(0, 50);
  for (const el of document.querySelectorAll('body *')) {
    if (!visible(el)) {
      continue;
    }
    const b = el.getBoundingClientRect();
    if (b.right > vw + 1 && !clippedByScroller(el)) {
      out.offRight.push(`${label(el)} (right ${Math.round(b.right)})`);
    }
    const ownText = [...el.childNodes].some((n) => n.nodeType === 3 && n.textContent?.trim());
    if (ownText) {
      const fs = Number.parseFloat(getComputedStyle(el).fontSize);
      if (fs < 12) {
        out.small.push(`${fs}px "${label(el)}"`);
      }
    }
    if (touch && el.matches('button, a[href], input, select, textarea, [role=button]')) {
      if (b.width < 24 || b.height < 24) {
        out.tiny.push(`${Math.round(b.width)}x${Math.round(b.height)} "${label(el)}"`);
      }
    }
  }
  const uniq = (a: string[]) => [...new Set(a)].slice(0, 8);
  return {
    hScroll: out.hScroll,
    offRight: uniq(out.offRight),
    small: uniq(out.small),
    tiny: uniq(out.tiny),
  };
}

// ── 시나리오 ──────────────────────────────────────────────────────────

async function runEnv(
  browser: Browser,
  env: Env,
  base: string,
  seeded: Seeded,
  out: string,
  add: (f: Finding) => void,
): Promise<{ passed: number; total: number }> {
  const ctx: BrowserContext = await browser.newContext({
    viewport: env.viewport,
    isMobile: !!env.isMobile,
    hasTouch: !!env.hasTouch,
    deviceScaleFactor: env.deviceScaleFactor,
    colorScheme: env.scheme,
  });
  const p: Page = await ctx.newPage();
  p.setDefaultTimeout(6000);
  p.on('pageerror', (e) =>
    add({ env: env.name, step: 'runtime', kind: 'JS 에러', detail: e.message.slice(0, 200) }),
  );
  p.on('console', (m) => {
    if (m.type() === 'error') {
      add({ env: env.name, step: 'console', kind: '콘솔 에러', detail: m.text().slice(0, 200) });
    }
  });

  const shot = (name: string) =>
    p.screenshot({
      path: join(out, `${env.name}-${name}.png`),
      animations: 'disabled',
      timeout: 15_000,
    });
  // 보기 탭 바가 DOM 에 둘(좁은/넓은 화면용)이다 — 보이는 쪽만.
  const tab = (name: string): Locator =>
    p.locator('nav[aria-label="보기"]:visible button', { hasText: name }).first();
  const dialog = () => p.getByRole('dialog');
  // 할 일 행의 드래그 핸들 이름에도 제목이 들어 있다 — 정확히 제목인 버튼만.
  const row = (title: string) => p.getByRole('button', { name: title, exact: true });

  let passed = 0;
  let total = 0;
  const step = async (name: string, fn: () => Promise<void>) => {
    total++;
    try {
      await fn();
      await p.waitForTimeout(350);
      const r = await p.evaluate(auditInPage, !!env.hasTouch);
      if (r.hScroll > 1) {
        add({ env: env.name, step: name, kind: '가로 스크롤', detail: `${r.hScroll}px 넘침` });
      }
      if (r.offRight.length) {
        add({ env: env.name, step: name, kind: '화면 밖 요소', detail: r.offRight.join(' | ') });
      }
      if (r.small.length) {
        add({ env: env.name, step: name, kind: '12px 미만 글자', detail: r.small.join(' | ') });
      }
      if (r.tiny.length) {
        add({ env: env.name, step: name, kind: '24px 미만 터치 타깃', detail: r.tiny.join(' | ') });
      }
      passed++;
      // 스크린샷은 기록일 뿐이라 늦어도(폰트·애니메이션 대기) 기능 실패로 치지 않는다.
      await shot(name).catch((e: unknown) =>
        add({
          env: env.name,
          step: name,
          kind: '스크린샷 실패',
          detail: (e instanceof Error ? e.message : String(e)).split('\n')[0]!.slice(0, 200),
        }),
      );
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      add({
        env: env.name,
        step: name,
        kind: '실패',
        detail: message.split('\n')[0]!.slice(0, 200),
      });
      await shot(`${name}-FAIL`).catch(() => {});
    }
  };

  const tag = `E2E-${env.name}`;
  const added = `${tag} 추가한 항목`;
  const renamed = `${tag} 제목 바꿈`;

  // 누르지 않는 버튼: "GitHub 이슈 만들기"(실제 GitHub 에 이슈를 연다), "새 세션 띄우기"(claude 세션을 띄운다),
  // "에이전트에게 보내기"(살아 있는 세션에 일을 넘긴다). 격리된 데몬이어도 이 셋은 바깥에 닿는다.

  await step('01-feed', async () => {
    await p.goto(`${base}/`);
    await p.getByRole('main', { name: '피드' }).waitFor();
  });
  await step('02-feed-expand-unread', async () => {
    const more = p.getByRole('button', { name: /읽지 않은 댓글 \d+건 더 보기/ });
    await more.click();
    if (await more.count()) {
      throw new Error('요약 줄을 눌러도 펼쳐지지 않음');
    }
  });
  await step('03-feed-row-open', async () => {
    await p.locator('section[aria-label="내 차례"] li button').first().click();
    await dialog().waitFor({ timeout: 3000 });
  });
  await step('04-drawer-esc', async () => {
    await p.keyboard.press('Escape');
    await p.waitForTimeout(300);
    if (await dialog().count()) {
      throw new Error('ESC 로 상세가 안 닫힘');
    }
  });
  await step('05-board-switcher', async () => {
    await p.getByRole('button', { name: /^보드 — 지금/ }).click();
    await p.waitForTimeout(300);
    await shot('05a-switcher-open').catch(() => {});
    await p.getByRole('menuitemradio', { name: 'Demo', exact: true }).click();
    await p.waitForURL(/\/demo(?:$|[/?#])/, { timeout: 3000 });
  });
  await step('06-todos-board', async () => {
    await tab('할 일').click();
    await row('설정 화면 정리').first().waitFor({ timeout: 3000 });
  });
  await step('07-quick-add', async () => {
    const box = p.getByRole('textbox', { name: /새 작업/ });
    await box.fill(added);
    await box.press('Enter');
    await row(added).first().waitFor({ timeout: 3000 });
  });
  await step('08-open-detail', async () => {
    await row(added).first().click();
    await dialog().waitFor({ timeout: 3000 });
  });
  await step('09-edit-title', async () => {
    await p.getByRole('button', { name: /^제목 수정/ }).click();
    const input = dialog().getByRole('textbox').first();
    await input.fill(renamed);
    await input.press('Enter');
    await dialog().getByText(renamed).first().waitFor({ timeout: 3000 });
  });
  await step('10-edit-description', async () => {
    const d = dialog();
    await d.locator('button.drawer-desc').click();
    await p.waitForTimeout(300);
    await d.locator('.cm-content, textarea').first().click();
    await p.keyboard.type('설명을 **적었다**');
    await shot('10a-editing').catch(() => {});
    await d.getByRole('button', { name: /^저장/ }).click();
    await d.getByText('적었다').first().waitFor({ timeout: 3000 });
  });
  await step('11-comment', async () => {
    const d = dialog();
    await d.getByRole('textbox', { name: /진행 상황이나 질문/ }).fill('E2E 댓글');
    await d.getByRole('button', { name: '등록' }).click();
    await d.getByText('E2E 댓글').first().waitFor({ timeout: 3000 });
  });
  await step('12-start-done', async () => {
    const d = dialog();
    await d.getByRole('button', { name: '시작', exact: true }).click();
    await d.getByRole('button', { name: '완료', exact: true }).click();
    await d.getByRole('button', { name: '시작', exact: true }).waitFor({ timeout: 3000 });
  });
  await step('13-close-and-done-collapsed', async () => {
    await p.getByRole('button', { name: '상세 닫기' }).click();
    await p.waitForTimeout(300);
    if (await row(renamed).count()) {
      throw new Error('완료한 항목이 접히지 않고 보임');
    }
  });
  await step('14-expand-done-and-archive', async () => {
    await p
      .getByRole('button', { name: /완료된 작업 \d+개/ })
      .first()
      .click();
    await row(renamed).first().click();
    // 상세 안 "보관" — 하위 작업·댓글 쪽 보관 버튼과 섞이지 않게 상태 버튼(.drawer-btn)으로 좁힌다.
    await dialog()
      .locator('.drawer-btn', { hasText: /^보관$/ })
      .click();
    await dialog().locator('.drawer-btn', { hasText: '보관 해제' }).waitFor({ timeout: 3000 });
  });
  await step('15-notes', async () => {
    await p.keyboard.press('Escape');
    await tab('노트').click();
    await p.getByText('회의 메모').first().waitFor({ timeout: 3000 });
  });
  await step('16-note-create', async () => {
    await p
      .getByRole('button', { name: /새 노트|노트 추가/ })
      .first()
      .click();
    await p.waitForTimeout(600);
    await p.locator('.cm-content, textarea').first().click();
    await p.keyboard.type(`${tag} 노트 본문`);
    await p.waitForTimeout(1500);
  });
  await step('17-note-back', async () => {
    await p
      .getByRole('button', { name: /목록|뒤로|닫기/ })
      .first()
      .click();
    await p.getByText(`${tag} 노트 본문`).first().waitFor({ timeout: 3000 });
  });
  await step('18-worklog', async () => {
    await tab('작업로그').click();
    await p.getByRole('main', { name: '작업로그' }).waitFor();
  });
  await step('19-worklog-search', async () => {
    await p.getByRole('textbox', { name: '작업로그 찾기' }).fill('demo');
    await p.waitForTimeout(800);
  });
  await step('20-worklog-stats', async () => {
    await p.getByRole('button', { name: /통계/ }).first().click();
    await p.waitForTimeout(1200);
  });
  await step('21-github', async () => {
    await tab('GitHub').click();
    await p.waitForTimeout(800);
  });
  await step('22-menu', async () => {
    await p.getByRole('button', { name: '메뉴' }).click();
    await p.getByRole('menu').waitFor({ timeout: 2000 });
  });
  await step('23-theme-toggle', async () => {
    const want = env.scheme === 'dark' ? 'light' : 'dark';
    await p.getByRole('menuitemradio', { name: want === 'dark' ? '다크' : '라이트' }).click();
    await p.waitForTimeout(300);
    const theme = await p.evaluate(() => document.documentElement.dataset.theme ?? '');
    if (theme && theme !== want) {
      throw new Error(`테마가 "${theme}" 로 남음`);
    }
  });
  await step('24-reload-keeps-tab', async () => {
    await p.keyboard.press('Escape');
    await tab('노트').click();
    await p.reload();
    await p.waitForTimeout(1200);
    const pressed = await p
      .locator('nav[aria-label="보기"]:visible button[aria-pressed="true"]')
      .first()
      .textContent();
    // 지금은 보기 탭을 주소·저장소에 싣지 않아 새로고침하면 첫 화면(피드)으로 돌아간다(store.ts 의 view 초기값).
    // 의도인지 결정이 안 났으니 기능 실패가 아니라 발견으로 남긴다.
    if (!/노트/.test(pressed ?? '')) {
      add({
        env: env.name,
        step: '24-reload-keeps-tab',
        kind: '동작 확인',
        detail: `새로고침 뒤 탭이 "${pressed}"`,
      });
    }
  });
  await step('25-permalink', async () => {
    await p.goto(`${base}/demo/${seeded.permalinkNumber}`);
    await dialog().waitFor({ timeout: 3000 });
    await dialog().getByText('로그인 화면 문구 다듬기').first().waitFor({ timeout: 3000 });
    await p.goBack();
    await p.waitForTimeout(500);
  });

  await ctx.close();
  return { passed, total };
}

async function launchBrowser(headed: boolean): Promise<Browser> {
  try {
    return await chromium.launch({ headless: !headed });
  } catch (first) {
    try {
      return await chromium.launch({ headless: !headed, channel: 'chrome' });
    } catch {
      const message = first instanceof Error ? first.message.split('\n')[0] : String(first);
      throw new Error(
        `브라우저를 띄우지 못했다 (${message}). \`bunx playwright-core install chromium\` 로 받거나 Google Chrome 을 설치한다.`,
      );
    }
  }
}

function printTable(findings: Finding[]): void {
  if (!findings.length) {
    return;
  }
  const rows = findings.map((f) => [f.env, f.step, f.kind, f.detail]);
  const head = ['환경', '단계', '종류', '내용'];
  const widths = head.map((h, i) =>
    Math.max(h.length, ...rows.map((r) => Math.min(r[i]!.length, 40))),
  );
  const fmt = (r: string[]) =>
    r.map((c, i) => (i === 3 ? c : c.slice(0, 40).padEnd(widths[i]!))).join(' │ ');
  console.log(fmt(head));
  console.log(widths.map((w, i) => '─'.repeat(i === 3 ? 4 : w)).join('─┼─'));
  for (const r of rows) {
    console.log(fmt(r));
  }
}

async function main(): Promise<number> {
  const opts = parseArgs(Bun.argv.slice(2));
  if (opts.build) {
    run(['bun', 'run', 'build:ui']);
    const cargo = Bun.which('cargo') ?? join(homedir(), '.cargo', 'bin', 'cargo');
    run([cargo, 'build', '-p', 'rockyd']);
  }
  const work = mkdtempSync(join(tmpdir(), 'rocky-e2e-'));
  const out = opts.out ?? mkdtempSync(join(tmpdir(), 'rocky-e2e-shots-'));
  mkdirSync(out, { recursive: true });

  const findings: Finding[] = [];
  const results: { env: string; passed: number; total: number }[] = [];
  let daemon: Daemon | undefined;
  let browser: Browser | undefined;
  const cleanup = async () => {
    await browser?.close().catch(() => {});
    if (daemon) {
      await stopDaemon(daemon);
    }
    rmSync(work, { recursive: true, force: true });
  };
  process.once('SIGINT', () => {
    void cleanup().finally(() => process.exit(130));
  });
  try {
    daemon = await startDaemon(work);
    const seeded = await seed(daemon.base);
    browser = await launchBrowser(opts.headed);
    for (const env of ENVS) {
      const r = await runEnv(browser, env, daemon.base, seeded, out, (f) => findings.push(f));
      results.push({ env: env.name, ...r });
    }
  } finally {
    await cleanup();
  }

  const unique = dedupe(findings);
  for (const r of results) {
    console.log(`${r.env}: ${r.passed}/${r.total} 단계 통과`);
  }
  const failures = unique.filter((f) => FAILURE_KINDS.has(f.kind));
  const notices = unique.filter((f) => !FAILURE_KINDS.has(f.kind));
  if (failures.length) {
    console.log(`\n✗ 기능 실패 ${failures.length}건`);
    printTable(failures);
  }
  if (notices.length) {
    console.log(
      `\n⚠ 화면 점검 발견 ${notices.length}건${opts.strict ? '' : ' (경고 — --strict 면 실패)'}`,
    );
    printTable(notices);
  }
  console.log(`\n스크린샷: ${out}`);
  return exitCodeOf(unique, opts.strict);
}

if (import.meta.main) {
  process.exit(await main());
}
