import { expect, tab, test } from './support/test';

test('새로고침해도 보던 탭을 유지한다', async ({ page }) => {
  await page.goto('/demo');
  await tab(page, '노트').click();
  // 보던 탭은 주소(`?view=notes`)에 실린다 — 새로고침해도 같은 탭이어야 한다(#293).
  await expect(page).toHaveURL(/[?&]view=notes/);
  await page.reload();
  await expect(
    page.locator('nav[aria-label="보기"]:visible button[aria-pressed="true"]').first(),
  ).toContainText('노트');
  await expect(page).toHaveURL(/[?&]view=notes/);
});

test('퍼머링크로 상세를 열고 뒤로 간다', async ({ page }) => {
  await page.goto('/demo?view=todos');
  await page.goto(`/demo/${process.env.E2E_PERMALINK}`);
  const dialog = page.getByRole('dialog');
  await expect(dialog).toBeVisible();
  await expect(dialog.getByText('로그인 화면 문구 다듬기').first()).toBeVisible();
  await page.goBack();
  await expect(page).toHaveURL(/\/demo\?view=todos$/);
  await expect(dialog).toHaveCount(0);
});
