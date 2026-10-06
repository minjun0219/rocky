import { audit } from './support/audit';
import { expect, row, test } from './support/test';

// 상세에서 누르지 않는 버튼: "GitHub 이슈 만들기"(실제 GitHub 에 이슈를 연다), "에이전트에게 보내기"(새 세션 — rc 서버를
// 띄우거나 — 이나 살아 있는 세션에 일을 넘긴다). 격리된 데몬이어도 이 둘은 바깥에 닿는다.

test('상세에서 제목·설명·댓글을 고친다', async ({ page, api, tag }) => {
  await api.todo({ title: tag });
  await page.goto('/demo?view=todos');
  await row(page, tag).first().click();
  const dialog = page.getByRole('dialog');
  await expect(dialog).toBeVisible();
  await audit(page, '상세');

  const renamed = `${tag} 바꿈`;
  await page.getByRole('button', { name: /^제목 수정/ }).click();
  const input = dialog.getByRole('textbox').first();
  await input.fill(renamed);
  await input.press('Enter');
  await expect(dialog.getByText(renamed).first()).toBeVisible();

  await dialog.locator('button.drawer-desc').click();
  await dialog.locator('.cm-content, textarea').first().click();
  await page.keyboard.type('설명을 **적었다**');
  await audit(page, '설명 편집');
  await dialog.getByRole('button', { name: /^저장/ }).click();
  await expect(dialog.getByText('적었다').first()).toBeVisible();

  await dialog.getByRole('textbox', { name: /진행 상황이나 질문/ }).fill(`${tag} 댓글`);
  await dialog.getByRole('button', { name: '등록' }).click();
  await expect(dialog.getByText(`${tag} 댓글`).first()).toBeVisible();
});

test('완료하면 목록에서 접히고, 펼쳐서 보관한다', async ({ page, api, tag }) => {
  await api.todo({ title: tag });
  await page.goto('/demo?view=todos');
  await row(page, tag).first().click();
  const dialog = page.getByRole('dialog');
  const done = dialog.getByRole('button', { name: '완료', exact: true });
  await expect(done).toBeVisible();
  // 착수는 에이전트가 남기는 신호라 상세에 "시작" 이 없다.
  await expect(dialog.getByRole('button', { name: '시작', exact: true })).toHaveCount(0);
  await done.click();
  // 완료하면 "완료" 버튼이 사라지고 다시 여는 버튼으로 바뀐다.
  await expect(done).toHaveCount(0);

  await page.getByRole('button', { name: '상세 닫기' }).click();
  await expect(row(page, tag)).toHaveCount(0);
  await audit(page, '완료 접힘');

  await page
    .getByRole('button', { name: /완료한 할 일 \d+개/ })
    .first()
    .click();
  await row(page, tag).first().click();
  // 상세 안 "보관" — 하위 작업·댓글 쪽 보관 버튼과 섞이지 않게 상태 버튼(.drawer-btn)으로 좁힌다.
  await dialog.locator('.drawer-btn', { hasText: /^보관$/ }).click();
  await expect(dialog.locator('.drawer-btn', { hasText: '보관 해제' })).toBeVisible();
});
