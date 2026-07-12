import { describe, expect, it } from 'bun:test';
import { chunkNotionMarkdown, extractActionItems, summarizeNotionChunks } from './notion-chunking';

const SAMPLE = `# 문서 요약
주문 기능 개선을 위한 기획.

## 요구사항
- 사용자는 주문 목록을 페이지네이션으로 조회할 수 있어야 한다.
- 관리자는 필수로 주문 상태를 변경할 수 있어야 한다.

## 화면 단위
- 주문 목록 화면: 검색 폼, 상태 필터, 테이블, 페이지네이션 버튼

## API 의존성
- GET /api/orders
- PATCH /api/orders/{id}/status

## TODO
- [ ] 주문 목록 API 연동
- [ ] 상태 변경 모달 구현

## 확인 필요 사항
- 취소 상태에서 환불 상태로 바로 전환 가능한가?
`;

describe('chunkNotionMarkdown', () => {
  it('chunks by heading and keeps line metadata', () => {
    const chunks = chunkNotionMarkdown(SAMPLE, { maxCharsPerChunk: 300 });
    expect(chunks.length).toBeGreaterThan(3);
    expect(chunks[0]?.id).toBe('chunk-001');
    expect(chunks[0]?.startLine).toBeGreaterThan(0);
    expect(chunks[0]?.endLine).toBeGreaterThanOrEqual(chunks[0]?.startLine ?? 0);
    expect(chunks.some((chunk) => /^#+\s/.test(chunk.text))).toBe(false);
  });

  it('ignores markdown headings inside fenced code blocks', () => {
    const chunks = chunkNotionMarkdown('# Top\n\n```\n# not heading\n```\n\n## Real\nbody');
    expect(chunks.some((chunk) => chunk.headingPath.includes('not heading'))).toBe(false);
    expect(chunks.some((chunk) => chunk.headingPath.includes('Real'))).toBe(true);
  });

  it('ignores headings inside ~~~ / longer-marker fences (not just ```)', () => {
    // 틸드 fence + 길이 4 backtick fence 안의 `#` 라인은 heading 이 아니어야 한다.
    const chunks = chunkNotionMarkdown(
      '# Top\n\n~~~\n# tilde not heading\n~~~\n\n````\n# long not heading\n````\n\n## Real\nbody',
    );
    expect(chunks.some((chunk) => chunk.headingPath.includes('tilde not heading'))).toBe(false);
    expect(chunks.some((chunk) => chunk.headingPath.includes('long not heading'))).toBe(false);
    expect(chunks.some((chunk) => chunk.headingPath.includes('Real'))).toBe(true);
  });

  it('normalizes CRLF input — no stray \\r in chunk text', () => {
    const chunks = chunkNotionMarkdown('# Top\r\n\r\n본문 한 줄\r\n둘째 줄\r\n');
    expect(chunks.length).toBeGreaterThan(0);
    expect(chunks.every((chunk) => !chunk.text.includes('\r'))).toBe(true);
  });

  it('hard-slices single lines longer than maxChars', () => {
    const chunks = chunkNotionMarkdown(`# Top\n${'x'.repeat(25)}`, {
      maxCharsPerChunk: 10,
    });
    expect(chunks.length).toBe(3);
    expect(chunks.every((chunk) => chunk.text.length <= 10)).toBe(true);
    expect(chunks.every((chunk) => chunk.startLine === 2 && chunk.endLine === 2)).toBe(true);
  });

  it('falls back to the default chunk size for non-positive or invalid maxChars', () => {
    for (const maxCharsPerChunk of [0, -1, Number.NaN]) {
      const chunks = chunkNotionMarkdown(`# Top\n${'x'.repeat(25)}`, {
        maxCharsPerChunk,
      });
      expect(chunks.length).toBe(1);
      expect(chunks[0]?.text).toBe('x'.repeat(25));
    }
  });

  it('returns metadata summaries without full chunk text', () => {
    const chunks = chunkNotionMarkdown(SAMPLE, { maxCharsPerChunk: 300 });
    const summaries = summarizeNotionChunks(chunks);
    expect(typeof summaries[0]?.preview).toBe('string');
    expect('text' in (summaries[0] ?? {})).toBe(false);
  });
});

describe('extractActionItems', () => {
  it('extracts requirements/screens/apis/todos/questions', () => {
    const chunks = chunkNotionMarkdown(SAMPLE, { maxCharsPerChunk: 300 });
    const extracted = extractActionItems(chunks);
    expect(extracted.requirements.some((x) => x.text.includes('페이지네이션'))).toBe(true);
    expect(extracted.screens.some((x) => x.text.includes('주문 목록 화면'))).toBe(true);
    expect(extracted.apis.some((x) => x.text === 'GET /api/orders')).toBe(true);
    expect(extracted.todos.some((x) => x.text.includes('주문 목록 API 연동'))).toBe(true);
    expect(extracted.questions.some((x) => x.text.includes('전환 가능한가'))).toBe(true);
  });

  it('does not extract APIs or TODOs from fenced code blocks', () => {
    const chunks = chunkNotionMarkdown(
      `## API\n\n\`\`\`ts\n// TODO: 예시 코드만 수정\nfetch("GET /api/example")\n\`\`\`\n\n- GET /api/orders\n- [ ] 주문 목록 API 연동`,
    );
    const extracted = extractActionItems(chunks);
    expect(extracted.apis.some((x) => x.text === 'GET /api/example')).toBe(false);
    expect(extracted.todos.some((x) => x.text.includes('예시 코드만 수정'))).toBe(false);
    expect(extracted.apis.some((x) => x.text === 'GET /api/orders')).toBe(true);
    expect(extracted.todos.some((x) => x.text.includes('주문 목록 API 연동'))).toBe(true);
  });

  it('strips ~~~ fenced code from extraction too (not just ```)', () => {
    const chunks = chunkNotionMarkdown(
      `## API\n\n~~~ts\n// TODO: 틸드 fence 예시\nfetch("GET /api/tilde")\n~~~\n\n- GET /api/orders`,
    );
    const extracted = extractActionItems(chunks);
    expect(extracted.apis.some((x) => x.text === 'GET /api/tilde')).toBe(false);
    expect(extracted.todos.some((x) => x.text.includes('틸드 fence 예시'))).toBe(false);
    expect(extracted.apis.some((x) => x.text === 'GET /api/orders')).toBe(true);
  });
});
