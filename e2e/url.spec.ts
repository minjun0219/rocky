import { expect, tab, test } from './support/test';

test('새로고침해도 보던 탭을 유지하고, 뒤로 가면 앞 탭', async ({ page }) => {
  await page.goto('/');
  await tab(page, '작업로그').click();
  // 보던 탭은 주소(`?view=worklog`)에 실린다 — 새로고침해도 같은 탭이어야 한다(#293).
  await expect(page).toHaveURL(/[?&]view=worklog/);
  await page.reload();
  await expect(
    page.locator('nav[aria-label="보기"]:visible button[aria-pressed="true"]').first(),
  ).toContainText('작업로그');
  await page.goBack();
  await expect(page.getByRole('main', { name: '피드' })).toBeVisible();
});

test('옛 보드 · 할 일 · 노트 주소는 피드로 열고 맨 주소로 고친다', async ({ page }) => {
  for (const old of ['/demo/12', '/demo/notes/3', '/?view=todos']) {
    await page.goto(old);
    await expect(page.getByRole('main', { name: '피드' })).toBeVisible();
    await expect(page).toHaveURL(/\/$/);
  }
});
