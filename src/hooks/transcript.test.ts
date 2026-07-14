import { describe, expect, it } from 'bun:test';
import { buildTurnContent, extractTurn } from './transcript';

const TRANSCRIPT = [
  {
    type: 'user',
    message: { role: 'user', content: [{ type: 'text', text: '엔드포인트 검색해줘' }] },
  },
  {
    type: 'assistant',
    message: {
      role: 'assistant',
      content: [
        { type: 'text', text: '검색합니다' },
        { type: 'tool_use', name: 'openapi_search', input: {} },
        { type: 'tool_use', name: 'openapi_search', input: {} },
      ],
    },
  },
  { type: 'user', message: { role: 'user', content: [{ type: 'tool_result', content: 'ok' }] } },
  {
    type: 'assistant',
    message: { role: 'assistant', content: [{ type: 'text', text: '3개 찾았습니다' }] },
  },
]
  .map((e) => JSON.stringify(e))
  .join('\n');

describe('extractTurn', () => {
  it('pulls last real user prompt, deduped tool names w/ count, final assistant text', () => {
    const parts = extractTurn(TRANSCRIPT);
    expect(parts?.req).toBe('엔드포인트 검색해줘');
    expect(parts?.tools).toEqual(['openapi_search(×2)']);
    expect(parts?.did).toBe('3개 찾았습니다');
  });
  it('ignores tool_result-only user messages as the prompt boundary', () => {
    expect(extractTurn(TRANSCRIPT)?.req).toBe('엔드포인트 검색해줘');
  });
  it('returns null when there is no user prompt', () => {
    const onlyAssistant = JSON.stringify({
      type: 'assistant',
      message: { role: 'assistant', content: [{ type: 'text', text: 'hi' }] },
    });
    expect(extractTurn(onlyAssistant)).toBeNull();
  });
  it('skips malformed lines gracefully', () => {
    expect(extractTurn(`not json\n${TRANSCRIPT}\n{"partial":`)?.req).toBe('엔드포인트 검색해줘');
  });
});

describe('buildTurnContent', () => {
  it('collapses whitespace and joins parts', () => {
    expect(buildTurnContent({ req: 'a  b\n\nc', tools: ['x', 'y'], did: 'done' }, 800)).toBe(
      'req: a b c | tools: x, y | did: done',
    );
  });
  it('truncates each field to maxChars with an ellipsis', () => {
    expect(buildTurnContent({ req: 'abcdefgh', tools: [], did: '' }, 4)).toBe(
      'req: abcd… | tools: (none) | did: (none)',
    );
  });
  it('shows (none) for empty parts and caps tools at 20', () => {
    const many = Array.from({ length: 25 }, (_, i) => `t${i}`);
    const s = buildTurnContent({ req: '', tools: many, did: '' }, 800);
    expect(s.startsWith('req: (none) | tools: t0, t1')).toBe(true);
    expect(s).toContain('did: (none)');
    expect(s.split('tools: ')[1]?.split(' | ')[0]?.split(', ').length).toBe(20);
  });
});
