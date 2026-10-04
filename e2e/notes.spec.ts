import { audit } from './support/audit';
import { expect, tab, test } from './support/test';

test('새 노트를 쓰고 뒤로가기로 상세를 닫는다', async ({ page, tag }) => {
  await page.goto('/demo');
  await tab(page, '노트').click();
  await expect(page.getByText('회의 메모').first()).toBeVisible();
  await audit(page, '노트 목록');

  await page
    .getByRole('button', { name: /새 노트|노트 추가/ })
    .first()
    .click();
  const detail = page.getByRole('region', { name: '노트 상세' });
  await expect(detail).toBeVisible();
  await detail.locator('.cm-content, textarea').first().click();
  await page.keyboard.type(`${tag} 본문`);
  await audit(page, '노트 편집');

  // 뒤로가기는 노트 상세 머리의 "‹ 노트" 버튼이다 — 편집기 서식 막대의 "목록"(글머리표)과 헷갈리지 않게
  // 이름을 정확히 고르고 "노트 상세" 영역 안에서만 찾는다(보기 탭 "노트 N" 과도 갈린다).
  await detail.getByRole('button', { name: '노트', exact: true }).click();
  await expect(detail).toHaveCount(0);
  // 목록 카드에 본문이 보이면 저장된 것이다.
  await expect(page.getByText(`${tag} 본문`).first()).toBeVisible({ timeout: 8000 });
});
