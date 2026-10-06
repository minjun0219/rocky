import { audit } from './support/audit';
import { expect, test } from './support/test';

test.beforeEach(async ({ page }) => {
  await page.goto('/');
  await expect(page.getByRole('main', { name: '피드' })).toBeVisible();
});

test('피드 첫 화면에 내 차례가 보인다', async ({ page }) => {
  await expect(page.locator('section[aria-label="내 차례"]')).toBeVisible();
  await audit(page, '피드');
});

test('읽지 않은 댓글 요약 줄을 펼친다', async ({ page }) => {
  // 새 브라우저는 댓글을 전부 안 읽은 상태다 — 픽스처의 댓글 5건이 "N건 더 보기" 로 접힌다.
  const more = page.getByRole('button', { name: /읽지 않은 댓글 \d+개 더 보기/ });
  await more.click();
  await expect(more).toHaveCount(0);
  await audit(page, '읽지 않은 댓글 펼침');
});

test('피드 행으로 상세를 열고 ESC 로 닫는다', async ({ page }) => {
  // 답 기다리는 에이전트 행(가짜 claude 픽스처)은 상세가 아니라 에이전트 탭을 연다 — 할 일 행을 누른다.
  await page
    .locator('section[aria-label="내 차례"] li button')
    .filter({ hasNotText: '답 기다림' })
    .first()
    .click();
  await expect(page.getByRole('dialog')).toBeVisible();
  await audit(page, '피드에서 연 상세');
  await page.keyboard.press('Escape');
  await expect(page.getByRole('dialog')).toHaveCount(0);
});
