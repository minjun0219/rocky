import { audit } from './support/audit';
import { expect, test } from './support/test';

test('메뉴에서 테마를 바꾼다', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: '메뉴' }).click();
  await expect(page.getByRole('menu')).toBeVisible();
  await audit(page, '메뉴');

  const want = test.info().project.use.colorScheme === 'dark' ? 'light' : 'dark';
  await page.getByRole('menuitemradio', { name: want === 'dark' ? '다크' : '라이트' }).click();
  await expect(page.locator('html')).toHaveAttribute('data-theme', want);
});
