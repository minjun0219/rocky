import { audit } from './support/audit';
import { expect, row, tab, test } from './support/test';

test('보드 목록에서 보드를 바꾼다', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: /^보드 — 지금/ }).click();
  // 보드 목록 항목은 menuitemradio 다.
  const demo = page.getByRole('menuitemradio', { name: 'Demo', exact: true });
  await expect(demo).toBeVisible();
  await audit(page, '보드 목록');
  await demo.click();
  await expect(page).toHaveURL(/\/demo(?:$|[/?#])/);
  await tab(page, '할 일').click();
  await expect(row(page, '설정 화면 정리').first()).toBeVisible();
  await audit(page, '할 일 보드');
});

test('빠른 추가로 할 일을 만든다', async ({ page, tag }) => {
  await page.goto('/demo?view=todos');
  const box = page.getByRole('textbox', { name: /새 작업/ });
  await box.fill(tag);
  await box.press('Enter');
  await expect(row(page, tag).first()).toBeVisible();
  await audit(page, '빠른 추가 뒤');
});
