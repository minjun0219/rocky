import { describe, expect, it } from 'bun:test';
import {
  botVerdict,
  ciStateOf,
  parseArgs,
  reactionsToRemove,
  readyVerdict,
  summarizeThreads,
  type ThreadNode,
  type ThreadSummary,
  transitionsBetween,
} from './pr-threads';

const ME = 'minjun0219';

function thread(
  over: Partial<ThreadNode> & { reactions?: Array<[string, string]> } = {},
): ThreadNode {
  const { reactions = [], ...rest } = over;
  return {
    id: 'PRRT_1',
    isResolved: false,
    isOutdated: false,
    path: 'a.rs',
    line: 3,
    comments: {
      nodes: [
        {
          id: 'PRRC_1',
          author: { login: 'chatgpt-codex-connector' },
          body: 'P1',
          createdAt: '2026-09-28T01:00:00Z',
          reactions: { nodes: reactions.map(([content, login]) => ({ content, user: { login } })) },
        },
      ],
    },
    ...rest,
  };
}

describe('summarizeThreads', () => {
  it('미해결만 남기고 첫 코멘트 id 와 내 상태 리액션을 편다', () => {
    const out = summarizeThreads(
      [
        thread({
          reactions: [
            ['EYES', ME],
            ['THUMBS_UP', 'someone-else'],
            ['HEART', ME],
          ],
        }),
        thread({ id: 'PRRT_2', isResolved: true }),
      ],
      ME,
    );
    expect(out).toHaveLength(1);
    expect(out[0]?.commentId).toBe('PRRC_1');
    // 남의 리액션과 상태 아닌 리액션(HEART)은 세지 않는다.
    expect(out[0]?.mine).toEqual(['EYES']);
    expect(out[0]?.author).toBe('chatgpt-codex-connector');
  });

  it('코멘트가 없는 스레드는 건너뛴다', () => {
    expect(summarizeThreads([thread({ comments: { nodes: [] } })], ME)).toEqual([]);
  });
});

describe('reactionsToRemove', () => {
  it('목표 외의 내 상태 리액션만 — 옛 👍 도 뗀다', () => {
    expect(reactionsToRemove(['EYES', 'THUMBS_UP', 'HEART'], 'ROCKET')).toEqual([
      'EYES',
      'THUMBS_UP',
    ]);
    expect(reactionsToRemove(['ROCKET'], 'ROCKET')).toEqual([]);
    expect(reactionsToRemove([], 'EYES')).toEqual([]);
  });
});

describe('botVerdict', () => {
  const head = '2026-09-28T01:00:00Z';
  const bot = (submittedAt: string | null) => ({
    author: { login: 'chatgpt-codex-connector' },
    submittedAt,
  });
  const thumbs = (login: string, createdAt: string, content = 'THUMBS_UP') => ({
    content,
    createdAt,
    user: { login },
  });

  it('head 이후 봇 리뷰 제출 = findings — 이전 리뷰·사람 리뷰·미제출은 아니다', () => {
    expect(botVerdict([bot('2026-09-28T01:05:00Z')], [], head)).toBe('findings');
    expect(botVerdict([bot('2026-09-28T00:55:00Z')], [], head)).toBe('pending');
    expect(botVerdict([bot(null)], [], head)).toBe('pending');
    expect(
      botVerdict(
        [{ author: { login: 'minjun0219' }, submittedAt: '2026-09-28T02:00:00Z' }],
        [],
        head,
      ),
    ).toBe('pending');
    expect(botVerdict([{ author: null, submittedAt: '2026-09-28T02:00:00Z' }], [], head)).toBe(
      'pending',
    );
  });

  it('본문의 봇 👍 = clean — 사람 👍·👀·head 이전 것은 아니다', () => {
    expect(botVerdict([], [thumbs('chatgpt-codex-connector', '2026-09-28T01:03:00Z')], head)).toBe(
      'clean',
    );
    // 실제 GraphQL 은 리액션 user 를 `[bot]` 접미사로 준다(#158 실측).
    expect(
      botVerdict([], [thumbs('chatgpt-codex-connector[bot]', '2026-09-28T01:03:00Z')], head),
    ).toBe('clean');
    expect(botVerdict([], [thumbs('minjun0219', '2026-09-28T01:03:00Z')], head)).toBe('pending');
    expect(
      botVerdict([], [thumbs('chatgpt-codex-connector', '2026-09-28T01:03:00Z', 'EYES')], head),
    ).toBe('pending');
    expect(botVerdict([], [thumbs('chatgpt-codex-connector', '2026-09-28T00:30:00Z')], head)).toBe(
      'pending',
    );
    expect(
      botVerdict(
        [],
        [{ content: 'THUMBS_UP', createdAt: '2026-09-28T02:00:00Z', user: null }],
        head,
      ),
    ).toBe('pending');
  });

  it('리뷰와 👍 가 둘 다 있으면 findings 가 이긴다', () => {
    expect(
      botVerdict(
        [bot('2026-09-28T01:05:00Z')],
        [thumbs('chatgpt-codex-connector', '2026-09-28T01:06:00Z')],
        head,
      ),
    ).toBe('findings');
  });
});

describe('parseArgs', () => {
  it('list / watch 는 PR 번호 생략 가능, --timeout 은 watch 에만 의미', () => {
    expect(parseArgs(['list'])).toEqual({ cmd: 'list', timeoutSec: 300, intervalSec: 60 });
    expect(parseArgs(['watch', '154', '--timeout', '60'])).toEqual({
      cmd: 'watch',
      pr: 154,
      timeoutSec: 60,
      intervalSec: 60,
    });
  });

  it('react 는 코멘트 id 와 상태 리액션 둘 다 필요하고 👍 는 거부한다', () => {
    expect(parseArgs(['react', 'PRRC_1', 'EYES'])).toMatchObject({
      cmd: 'react',
      commentId: 'PRRC_1',
      reaction: 'EYES',
    });
    expect(() => parseArgs(['react', 'PRRC_1'])).toThrow('둘 다');
    expect(() => parseArgs(['react', 'PRRC_1', 'THUMBS_UP'])).toThrow('EYES|ROCKET');
  });

  it('잘못된 입력은 조용히 넘기지 않는다', () => {
    expect(() => parseArgs(['list', 'abc'])).toThrow('PR 번호');
    expect(() => parseArgs(['watch', '--timeout', '0'])).toThrow('--timeout');
    expect(() => parseArgs(['list', '--nope'])).toThrow('모르는 옵션');
    expect(() => parseArgs([])).toThrow('사용법');
  });
});

describe('botVerdict — 리뷰의 커밋으로 head 대조', () => {
  const codex = { login: 'chatgpt-codex-connector' };
  it('커밋이 실린 리뷰는 시각이 아니라 커밋으로 판정한다', () => {
    // 서버 리베이스로 head 가 새로 생겼다(시각은 리뷰보다 앞) — 옛 리뷰를 새것으로 오인하면 안 된다.
    const reviews = [
      { author: codex, submittedAt: '2026-09-28T09:00:00Z', commit: { oid: 'aaaaaaa111' } },
    ];
    expect(botVerdict(reviews, [], '2026-09-28T08:00:00Z', 'bbbbbbb')).toBe('pending');
    expect(botVerdict(reviews, [], '2026-09-28T08:00:00Z', 'aaaaaaa')).toBe('findings');
  });
  it('커밋이 없는 리뷰는 예전대로 시각으로 본다', () => {
    const reviews = [{ author: codex, submittedAt: '2026-09-28T09:00:00Z' }];
    expect(botVerdict(reviews, [], '2026-09-28T08:00:00Z', 'bbbbbbb')).toBe('findings');
  });
});

describe('readyVerdict', () => {
  const t = (mine: string[]): ThreadSummary => ({
    threadId: 'T',
    commentId: 'C',
    path: 'a',
    line: 1,
    outdated: false,
    author: 'chatgpt-codex-connector',
    createdAt: '2026-09-28T00:00:00Z',
    mine,
    body: '',
  });
  it('CI 초록 + 전부 👀 면 ready — 열린 스레드 수는 조건이 아니다', () => {
    const v = readyVerdict([t(['EYES']), t(['EYES'])], 'pass');
    expect(v.ready).toBe(true);
    expect(v.threads).toEqual({ total: 2, unhandled: 0, rocket: 0 });
  });
  it('👀 없는 스레드, 🚀, CI 실패·진행 중은 각각 이유가 된다', () => {
    const v = readyVerdict([t([]), t(['ROCKET']), t(['EYES'])], 'fail');
    expect(v.ready).toBe(false);
    expect(v.threads).toEqual({ total: 3, unhandled: 1, rocket: 1 });
    expect(v.reasons.join(' ')).toMatch(/CI 실패/);
    expect(v.reasons.join(' ')).toMatch(/처리 안 된 스레드 1건/);
    expect(v.reasons.join(' ')).toMatch(/🚀 1건/);
    expect(readyVerdict([], 'pending').reasons).toEqual(['CI 진행 중']);
  });
});

describe('ciStateOf', () => {
  it('fail 이 하나라도 있으면 fail, pending 이 있으면 pending, 아니면 pass(skipping 포함)', () => {
    expect(ciStateOf(['a\tpass\t1s\turl', 'b\tskipping\t\turl'])).toBe('pass');
    expect(ciStateOf(['a\tpass', 'b\tpending'])).toBe('pending');
    expect(ciStateOf(['a\tfail', 'b\tpending'])).toBe('fail');
    expect(ciStateOf([])).toBe('pass');
  });
});

describe('transitionsBetween', () => {
  it('머지·닫힘·충돌 전이만 낸다 — 새 PR 과 리뷰 상태 변화는 무시', () => {
    const prev = [
      { number: 1, state: 'OPEN', mergeState: 'BLOCKED' },
      { number: 2, state: 'OPEN', mergeState: 'CLEAN' },
      { number: 3, state: 'OPEN', mergeState: 'CLEAN' },
    ];
    const cur = [
      { number: 1, state: 'MERGED', mergeState: 'UNKNOWN' },
      { number: 2, state: 'OPEN', mergeState: 'DIRTY' },
      { number: 3, state: 'OPEN', mergeState: 'BLOCKED' },
      { number: 4, state: 'OPEN', mergeState: 'DIRTY' },
    ];
    expect(transitionsBetween(prev, cur)).toEqual(['#1 MERGED', '#2 OPEN DIRTY']);
    expect(transitionsBetween(cur, cur)).toEqual([]);
  });
});

describe('parseArgs — ready / transitions', () => {
  it('ready 는 PR 번호를 받고, transitions 는 받지 않는다', () => {
    expect(parseArgs(['ready', '184']).pr).toBe(184);
    expect(parseArgs(['transitions', '--interval', '30']).intervalSec).toBe(30);
    expect(() => parseArgs(['transitions', '5'])).toThrow(/PR 번호를 받지 않는다/);
    expect(() => parseArgs(['transitions', '--interval', '0'])).toThrow(/양수/);
  });
});
