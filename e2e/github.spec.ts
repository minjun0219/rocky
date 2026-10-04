import { audit } from './support/audit';
import { expect, tab, test } from './support/test';

// 격리 데몬은 gh 설정이 없는 HOME 으로 돈다 — GitHub 탭은 빈 상태로 그려지기만 하면 된다.
test('GitHub 탭을 연다', async ({ page }) => {
  await page.goto('/demo');
  await tab(page, 'GitHub').click();
  await expect(page.getByRole('main', { name: 'GitHub' })).toBeVisible();
  await audit(page, 'GitHub');
});
