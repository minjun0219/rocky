import type { Page } from '@playwright/test';
import { expect, row, test } from './support/test';

// 보드 싱크(로컬 퍼스트 웹) — 화면이 보드를 사본에서 그리고, 바뀐 행이 소켓으로 밀려온다.

/** 보드 묶음을 다시 읽는 요청 — 사본으로 그리는 동안에는 없어야 한다. */
const LIST_PATHS = ['/api/todos', '/api/notes', '/api/sections', '/api/handoffs', '/api/boards'];

function recordListRequests(page: Page): string[] {
  const listed: string[] = [];
  page.on('request', (request) => {
    const path = new URL(request.url()).pathname;
    if (LIST_PATHS.includes(path)) {
      listed.push(path);
    }
  });
  return listed;
}

/** 이 탭이 사본으로 그리기 시작할 때까지 — 스토어가 `<html data-sync="replica">` 를 단다. */
async function replicaReady(page: Page): Promise<void> {
  await expect(page.locator('html[data-sync="replica"]')).toHaveCount(1);
}

test('다른 곳에서 만든 할 일이 다시 읽기 없이 밀려온다', async ({ page, api, tag }) => {
  await page.goto('/demo?view=todos');
  await replicaReady(page);
  const listed = recordListRequests(page);
  await api.todo({ title: tag });
  await expect(row(page, tag).first()).toBeVisible();
  expect(listed).toEqual([]);
});

test('다시 연 화면은 저장한 사본으로 그리고 할 일 목록을 다시 받지 않는다', async ({
  page,
  api,
  tag,
}) => {
  await api.todo({ title: tag });
  await page.goto('/demo?view=todos');
  await replicaReady(page);
  await expect(row(page, tag).first()).toBeVisible();
  // 사본이 IndexedDB 에 쓰일 틈 — 쓰기는 행을 그린 직후에 시작한다.
  await page.waitForTimeout(300);
  const listed = recordListRequests(page);
  await page.reload();
  await expect(row(page, tag).first()).toBeVisible();
  await replicaReady(page);
  await page.waitForTimeout(500);
  expect(listed).toEqual([]);
});
