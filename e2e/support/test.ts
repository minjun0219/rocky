/**
 * spec 이 쓰는 `test` — 격리 데몬 주소를 baseURL 로 꽂고, 페이지의 JS 에러·콘솔 에러를 실패로 친다.
 *
 * 셀렉터 함정(옛 `scripts/e2e/web.ts` 에서 옮김):
 * - 보기 탭 바(`nav[aria-label="보기"]`)가 DOM 에 둘(좁은/넓은 화면용)이다 — `:visible` 로 보이는 쪽만.
 * - 할 일 행의 드래그 핸들 이름에도 제목이 들어 있다 — 행은 `exact: true` 로 정확히 제목인 버튼만.
 */
import { test as base, expect, type Locator, type Page } from '@playwright/test';
import { call, type Todo } from './daemon';

type Fixtures = {
  /** 이 테스트 전용 항목 제목에 붙이는 꼬리표 — 같은 데몬을 병렬로 쓰는 테스트끼리 겹치지 않게. */
  tag: string;
  /** 픽스처 REST — 테스트가 자기 항목을 만든다. */
  api: {
    todo: (body: Record<string, unknown>) => Promise<Todo>;
  };
};

export const test = base.extend<Fixtures>({
  // biome-ignore lint/correctness/noEmptyPattern: Playwright 는 픽스처 함수의 첫 인자가 구조 분해여야 한다.
  baseURL: async ({}, use) => {
    const url = process.env.E2E_BASE_URL;
    if (!url) {
      throw new Error('E2E_BASE_URL 이 없다 — globalSetup 이 데몬을 띄우지 못했다');
    }
    await use(url);
  },
  // 재시도도 같은 데몬을 쓴다 — 시도마다 이름을 달리해, 앞 시도가 반쯤 바꿔 둔 항목을 다시 집지 않게.
  // biome-ignore lint/correctness/noEmptyPattern: Playwright 는 픽스처 함수의 첫 인자가 구조 분해여야 한다.
  tag: async ({}, use, info) => {
    const attempt = info.retry > 0 ? ` r${info.retry}` : '';
    await use(`E2E ${info.project.name} ${info.title}${attempt}`);
  },
  api: async ({ baseURL }, use) => {
    await use({
      todo: (body) => call<Todo>(baseURL!, 'POST', '/api/todos', { board: 'demo', ...body }),
    });
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

/** 정확히 그 제목인 할 일 행. */
export const row = (page: Page, title: string): Locator =>
  page.getByRole('button', { name: title, exact: true });
