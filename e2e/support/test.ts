/**
 * spec 이 쓰는 `test` — 격리 데몬 주소를 baseURL 로 꽂고, 페이지의 JS 에러·콘솔 에러를 실패로 친다.
 *
 * 셀렉터 함정(옛 `scripts/e2e/web.ts` 에서 옮김):
 * - 보기 탭 바(`nav[aria-label="보기"]`)가 DOM 에 둘(좁은/넓은 화면용)이다 — `:visible` 로 보이는 쪽만.
 */
import { test as base, expect, type Locator, type Page } from '@playwright/test';

export const test = base.extend({
  // biome-ignore lint/correctness/noEmptyPattern: Playwright 는 픽스처 함수의 첫 인자가 구조 분해여야 한다.
  baseURL: async ({}, use) => {
    const url = process.env.E2E_BASE_URL;
    if (!url) {
      throw new Error('E2E_BASE_URL 이 없다 — globalSetup 이 데몬을 띄우지 못했다');
    }
    await use(url);
  },
  page: async ({ page }, use) => {
    const errors: string[] = [];
    page.on('pageerror', (e) => errors.push(`JS 에러: ${e.message.slice(0, 200)}`));
    page.on('console', (m) => {
      if (m.type() === 'error') {
        errors.push(`콘솔 에러: ${m.text().slice(0, 200)}`);
      }
    });
    page.setDefaultTimeout(6000);
    await use(page);
    expect(errors, '페이지에서 난 에러').toEqual([]);
  },
});

export { expect };

/** 보이는 보기 탭 버튼. */
export const tab = (page: Page, name: string): Locator =>
  page.locator('nav[aria-label="보기"]:visible button', { hasText: name }).first();
