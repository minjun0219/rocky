/**
 * 화면 점검 — 가로 넘침, 화면 밖 요소, 12px 미만 글자, (터치) 24px 미만 타깃.
 *
 * 발견은 기능 실패가 아니다: 기본은 `test.info().annotations` 경고(+ 콘솔 한 줄)로만 남기고,
 * `E2E_STRICT=1` 이면 soft 실패로 친다(테스트는 끝까지 돌고 결과만 빨개진다).
 */
import { expect, type Page, test } from '@playwright/test';

export type AuditResult = { hScroll: number; offRight: string[]; small: string[]; tiny: string[] };

/**
 * 브라우저 안에서 도는 점검 — `page.evaluate` 로 넘기므로 바깥 변수를 쓰지 않는다.
 *
 * 터치 타깃은 **히트 영역**으로 잰다: 보이는 크기가 작아도 감싼 `<label>` 이나 절대 배치한
 * `::before`/`::after`(예: 번호 버튼의 `inset: 0 -12px`)가 넓혀 둔 영역이 24px 이상이면 통과다.
 * 행 체크박스(17px, 44px label 안)·번호 복사(15px 폭, ::after 로 44px)가 이 경우다.
 */
export function auditInPage({ touch, exempt }: { touch: boolean; exempt: string }): AuditResult {
  const vw = window.innerWidth;
  const out: AuditResult = {
    hScroll: document.documentElement.scrollWidth - vw,
    offRight: [],
    small: [],
    tiny: [],
  };
  const visible = (el: Element) => {
    const cs = getComputedStyle(el);
    if (cs.visibility === 'hidden' || cs.display === 'none' || Number(cs.opacity) === 0) {
      return false;
    }
    const b = el.getBoundingClientRect();
    return b.width > 0 && b.height > 0;
  };
  const clippedByScroller = (el: Element) => {
    for (let a = el.parentElement; a; a = a.parentElement) {
      const o = getComputedStyle(a).overflowX;
      if (o === 'auto' || o === 'scroll' || o === 'hidden' || o === 'clip') {
        return true;
      }
    }
    return false;
  };
  const label = (el: Element) =>
    String(el.getAttribute('aria-label') || el.textContent || el.className || el.tagName)
      .trim()
      .slice(0, 50);
  /** 보이는 상자 ∪ 감싼 label ∪ 절대 배치 가상 요소가 넓힌 상자. */
  const hitBox = (el: Element) => {
    const b = el.getBoundingClientRect();
    let w = b.width;
    let h = b.height;
    for (const pseudo of ['::before', '::after']) {
      const ps = getComputedStyle(el, pseudo);
      if (ps.content === 'none' || ps.content === 'normal' || ps.position !== 'absolute') {
        continue;
      }
      const px = (v: string) => (v === 'auto' ? Number.NaN : Number.parseFloat(v));
      const [t, r, bt, l] = [px(ps.top), px(ps.right), px(ps.bottom), px(ps.left)];
      if (![t, r, bt, l].some(Number.isNaN)) {
        w = Math.max(w, b.width - l - r);
        h = Math.max(h, b.height - t - bt);
      }
    }
    const wrap = el.closest('label');
    if (wrap && wrap !== el) {
      const lb = wrap.getBoundingClientRect();
      w = Math.max(w, lb.width);
      h = Math.max(h, lb.height);
    }
    return { w, h };
  };
  for (const el of document.querySelectorAll('body *')) {
    if (!visible(el)) {
      continue;
    }
    const b = el.getBoundingClientRect();
    if (b.right > vw + 1 && !clippedByScroller(el)) {
      out.offRight.push(`${label(el)} (right ${Math.round(b.right)})`);
    }
    const ownText = [...el.childNodes].some((n) => n.nodeType === 3 && n.textContent?.trim());
    if (ownText) {
      const fs = Number.parseFloat(getComputedStyle(el).fontSize);
      if (fs < 12) {
        out.small.push(`${fs}px "${label(el)}"`);
      }
    }
    if (
      touch &&
      el.matches('button, a[href], input, select, textarea, [role=button]') &&
      !el.matches(exempt)
    ) {
      const hit = hitBox(el);
      if (hit.w < 24 || hit.h < 24) {
        out.tiny.push(`${Math.round(hit.w)}x${Math.round(hit.h)} "${label(el)}"`);
      }
    }
  }
  const uniq = (a: string[]) => [...new Set(a)].slice(0, 8);
  return {
    hScroll: out.hScroll,
    offRight: uniq(out.offRight),
    small: uniq(out.small),
    tiny: uniq(out.tiny),
  };
}

/** 점검 결과를 사람이 읽는 발견 목록으로. */
export function describeFindings(r: AuditResult): string[] {
  const found: string[] = [];
  if (r.hScroll > 1) {
    found.push(`가로 스크롤: ${r.hScroll}px 넘침`);
  }
  if (r.offRight.length) {
    found.push(`화면 밖 요소: ${r.offRight.join(' | ')}`);
  }
  if (r.small.length) {
    found.push(`12px 미만 글자: ${r.small.join(' | ')}`);
  }
  if (r.tiny.length) {
    found.push(`24px 미만 터치 타깃: ${r.tiny.join(' | ')}`);
  }
  return found;
}

/**
 * 터치 타깃 점검에서 빼는 것 — 일부러 작게 둔 메타 칩. `web/styles/responsive.css` 의 "메타 칩 전반은 여기서
 * 제외한다" 주석: 44px 바닥을 주면 줄바꿈된 칩 줄이 행 높이를 두 배로 늘리고, 칩이 여는 대상(드로어·이슈)은
 * 44px 인 제목·번호나 드로어 안에서도 닿는다. 칩을 접는 백로그가 끝나면 여기서 뺀다.
 */
export const TOUCH_EXEMPT = '.comment-badge, .chip-link';

export const strict = process.env.E2E_STRICT === '1';

/** 지금 화면을 점검한다. `step` 은 발견을 어느 화면에서 봤는지 적는 이름. */
export async function audit(page: Page, step: string): Promise<void> {
  // 전환 애니메이션이 끝난 뒤 잰다.
  await page.waitForTimeout(350);
  const touch = !!test.info().project.use.hasTouch;
  const found = describeFindings(await page.evaluate(auditInPage, { touch, exempt: TOUCH_EXEMPT }));
  for (const detail of found) {
    const description = `${step} — ${detail}`;
    if (strict) {
      expect.soft(description, '화면 점검 발견(E2E_STRICT=1)').toBe('');
    } else {
      test.info().annotations.push({ type: '화면 점검', description });
      console.warn(`⚠ [${test.info().project.name}] ${description}`);
    }
  }
}
