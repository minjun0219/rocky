import { audit } from './support/audit';
import { expect, test } from './support/test';

test('피드 첫 화면에 내 차례가 보인다', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByRole('main', { name: '피드' })).toBeVisible();
  await expect(page.locator('section[aria-label="내 차례"]')).toBeVisible();
  await audit(page, '피드');
});
