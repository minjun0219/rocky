import type { Page } from '@playwright/test';
import { audit } from './support/audit';
import { AGENTS } from './support/claude';
import { expect, tab, test } from './support/test';

// 세션은 가짜 `claude` 픽스처(`support/claude.ts`)다. 행의 "메시지"·"멈추기" 버튼은 누르지 않는다 — 세션을 움직이는 버튼이다.

const agentsPane = (page: Page) => page.getByRole('main', { name: '에이전트' });

/** 피드 "내 차례" 의 답 기다리는 에이전트 행. */
const waitingRow = (page: Page, text: string) =>
  page
    .getByRole('main', { name: '피드' })
    .locator('section[aria-label="내 차례"] li button', { hasText: text });

test('에이전트 탭이 세션을 내 차례·실행 중·쉬는 중으로 묶는다', async ({ page }) => {
  await page.goto('/');
  await tab(page, '에이전트').click();
  await expect(page).toHaveURL(/view=agents/);
  const pane = agentsPane(page);
  const group = (title: string) => pane.locator(`section[aria-label="${title}"]`);
  await expect(group('내 차례').locator('li')).toHaveCount(2);
  await expect(group('실행 중').locator('li')).toHaveCount(2);
  await expect(group('쉬는 중').locator('li')).toHaveCount(3);
  // 작업 요약 줄 — 내 차례는 기다리는 것(needs), 실행 중은 하는 일(detail).
  await expect(group('내 차례').locator('li').first()).toContainText(AGENTS.blocked.needs);
  await expect(group('실행 중')).toContainText(AGENTS.working.detail);
  // 멈추기는 살아 있는 background 행에만, attach 복사는 background 행 전부(잠든 행 포함).
  const working = group('실행 중').locator('li', { hasText: AGENTS.working.name });
  await expect(working.getByRole('button', { name: '멈추기' })).toBeVisible();
  await expect(working.getByRole('button', { name: 'claude attach e2ewrk01 복사' })).toBeVisible();
  await expect(group('내 차례').getByRole('button', { name: '멈추기' })).toHaveCount(0);
  await expect(group('내 차례').getByRole('button', { name: /^claude attach / })).toHaveCount(2);
  await audit(page, '에이전트 탭');
});

test('보드를 고르면 그 보드의 세션만 — 워크트리 세션은 레포 보드로 접힌다', async ({ page }) => {
  await page.goto('/demo?view=agents');
  const pane = agentsPane(page);
  await expect(pane.locator('li')).toHaveCount(3);
  await expect(pane.locator('li', { hasText: AGENTS.worktree.name })).toBeVisible();
  await expect(pane.locator('li', { hasText: AGENTS.working.name })).toHaveCount(0);
});

test('피드의 "답 기다림" 행을 누르면 에이전트 탭으로 간다', async ({ page }) => {
  await page.goto('/');
  const waiting = waitingRow(page, AGENTS.blocked.needs);
  await expect(waiting).toContainText('답 기다림');
  // 요약이 없는 세션도 행이 된다 — 제목은 기다린다는 말로 채운다.
  await expect(waitingRow(page, AGENTS.blockedBare.name)).toContainText('답을 기다려요');
  await waiting.click();
  await expect(agentsPane(page)).toBeVisible();
  await expect(page).toHaveURL(/view=agents/);
});

test('메뉴에서 에이전트 탭을 끄면 피드의 답 기다림 행도 빠진다', async ({ page }) => {
  await page.goto('/');
  // 먼저 행이 뜬 것을 본다 — 세션 목록을 받기 전이라 없는 것과 헷갈리지 않게.
  await expect(waitingRow(page, AGENTS.blocked.needs)).toBeVisible();

  await page.getByRole('button', { name: '메뉴' }).click();
  await page.getByRole('checkbox', { name: '에이전트 탭 보기' }).uncheck();
  await expect(tab(page, '에이전트')).toHaveCount(0);
  await expect(waitingRow(page, '답 기다림')).toHaveCount(0);

  // 끈 것은 브라우저에 남는다 — 에이전트 탭 주소로 와도 피드를 연다.
  await page.goto('/?view=agents');
  await expect(page.getByRole('main', { name: '피드' })).toBeVisible();
  await expect(agentsPane(page)).toHaveCount(0);
});
