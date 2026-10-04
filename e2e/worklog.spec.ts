import { audit } from './support/audit';
import { expect, tab, test } from './support/test';

test('작업로그를 찾고 통계를 연다', async ({ page }) => {
  await page.goto('/demo');
  await tab(page, '작업로그').click();
  await expect(page.getByRole('main', { name: '작업로그' })).toBeVisible();
  await audit(page, '작업로그');

  const search = page.getByRole('textbox', { name: '작업로그 찾기' });
  await search.fill('demo');
  await expect(search).toHaveValue('demo');
  await audit(page, '작업로그 검색');

  await page.getByRole('button', { name: /통계/ }).first().click();
  await audit(page, '작업로그 통계');
});
