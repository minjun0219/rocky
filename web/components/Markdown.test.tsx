import { describe, expect, test } from 'bun:test';
import { renderToStaticMarkup } from 'react-dom/server';
import { Markdown, markdownExcerpt, safeHref } from './Markdown';

const html = (text: string) =>
  renderToStaticMarkup(<Markdown text={text} />).replace(/^<div class="md ">|<\/div>$/g, '');

describe('Markdown — 예전 줄 단위 정규식이 깨뜨리던 것', () => {
  // 실제 본문 262건 중 62건이 이런 식으로 깨졌다(굵게 누락 68 · 코드 누락 70 · `)` 붙은 링크).
  test('굵게 안의 코드', () => {
    expect(html('**chat.update 는 `markdown_text` 를 받는다**')).toBe(
      '<p><strong>chat.update 는 <code>markdown_text</code> 를 받는다</strong></p>',
    );
  });

  // 알려진 한계: `**findBy* 로 …**` 처럼 굵게 안에 짝 없는 `*` 가 있으면 CommonMark 규칙대로
  // 기울임으로 읽힌다(mdwire 는 고쳐 읽는다). 편집기 하이라이트도 같은 규칙이라 둘은 어긋나지 않는다.

  test('인라인 코드 뒤 공백을 지킨다', () => {
    expect(html('`MilestoneInput` 을 쓴다')).toBe('<p><code>MilestoneInput</code> 을 쓴다</p>');
    expect(html('**`as never` 캐스트 제거**')).toBe(
      '<p><strong><code>as never</code> 캐스트 제거</strong></p>',
    );
    expect(html('`a` `b`')).toBe('<p><code>a</code> <code>b</code></p>');
  });

  test('[글자](주소) — 닫는 괄호가 주소에 붙지 않는다', () => {
    expect(html('[문서](https://x.y/a) 끝')).toBe(
      '<p><a href="https://x.y/a" target="_blank" rel="noreferrer">문서</a> 끝</p>',
    );
  });

  test('줄을 넘는 굵게 — 줄바꿈은 그대로 줄을 나눈다', () => {
    expect(html('**첫 줄\n둘째 줄**')).toBe('<p><strong>첫 줄<br/>둘째 줄</strong></p>');
  });
});

describe('Markdown — 블록', () => {
  test('제목 · 목록 · 체크박스', () => {
    const out = html('# 이번 주\n\n- 하나\n- [x] 끝\n- [ ] 남음');
    expect(out).toContain('<h1>이번 주</h1>');
    expect(out).toContain('<li><p>하나</p></li>');
    expect(out).toContain('class="md-task is-done"');
    expect(out).toContain('☑');
    expect(out).toContain('☐');
    expect(out).not.toContain('[x]');
  });

  test('번호 목록은 시작 번호를 지킨다', () => {
    expect(html('3. 셋\n4. 넷')).toContain('<ol start="3">');
  });

  test('코드 블록 안의 기호는 글자 그대로', () => {
    expect(html('```\n# 주석\n**x**\n```')).toBe('<pre><code># 주석\n**x**</code></pre>');
  });

  test('표', () => {
    const out = html('| a | b |\n|---|---|\n| 1 | **2** |');
    expect(out).toContain('<th>a</th>');
    expect(out).toContain('<td><strong>2</strong></td>');
  });

  test('인용의 이어지는 줄', () => {
    expect(html('> 하나\n> 둘')).toBe('<blockquote><p>하나<br/>둘</p></blockquote>');
  });
});

describe('Markdown — 안전', () => {
  test('javascript: 링크는 걸지 않는다', () => {
    const out = html('[눌러](javascript:alert(1))');
    expect(out).not.toContain('href');
    expect(out).toContain('눌러');
  });

  test('HTML 태그는 글자로 — <br> 만 줄바꿈', () => {
    expect(html('a<script>x</script>b<br>c')).toBe('<p>a&lt;script&gt;x&lt;/script&gt;b<br/>c</p>');
  });

  test('이미지는 불러오지 않고 링크로', () => {
    const out = html('![그림](https://x.y/a.png)');
    expect(out).not.toContain('<img');
    expect(out).toContain('href="https://x.y/a.png"');
  });

  test('safeHref', () => {
    expect(safeHref('https://a.b')).toBe('https://a.b');
    expect(safeHref('www.a.b')).toBe('https://www.a.b');
    expect(safeHref('JavaScript:x')).toBeNull();
    expect(safeHref('data:text/html,x')).toBeNull();
  });
});

describe('markdownExcerpt', () => {
  test('첫 내용 줄에서 기호를 걷는다', () => {
    expect(markdownExcerpt('\n## **회의** 메모\n본문')).toBe('회의 메모');
    expect(markdownExcerpt('- [ ] [문서](https://x.y) 읽기')).toBe('문서 읽기');
    expect(markdownExcerpt('`rocky upgrade` 실측')).toBe('rocky upgrade 실측');
    expect(markdownExcerpt('')).toBe('');
  });

  test('길면 자른다', () => {
    expect(markdownExcerpt('가'.repeat(100), 10)).toBe(`${'가'.repeat(9)}…`);
  });
});
