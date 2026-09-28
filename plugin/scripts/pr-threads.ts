/**
 * PR 리뷰 스레드의 수집·리액션·대기를 한 곳에 — `/rocky:resolve-reviews` 가 쓴다.
 *
 * 매번 에이전트가 GraphQL 쿼리와 폴링 루프를 손으로 조립하면 페이지네이션을 빼먹거나
 * 리액션을 겹쳐 달거나(규약은 스레드당 하나) 변수 미사용 같은 사소한 실수로 한 번 더
 * 돈다. 여기서는 세 동작만 한다 — 스레드를 나열하고, 리액션을 바꿔 달고, CI 와 봇
 * 재리뷰가 올 때까지 기다린다. 코멘트·resolve·머지는 **하지 않는다**(원칙 4).
 *
 * ```
 * bun scripts/pr-threads.ts list 154            # 미해결 스레드 JSON (첫 코멘트 id·내 리액션 포함)
 * bun scripts/pr-threads.ts react PRRC_… EYES   # 내 다른 상태 리액션을 떼고 👀 하나만 남긴다
 * bun scripts/pr-threads.ts watch 154 --timeout 300
 *   # CI 가 끝날 때까지 → 현재 head 이후 봇 신호(리뷰 코멘트 또는 본문 👍)가 올 때까지(또는 timeout) 기다린 뒤 list 출력
 *   # verdict: findings(리뷰 제출) / clean(👍 만 — 지적 없음) / pending(안 옴)
 * ```
 *
 * PR 번호를 생략하면 현재 브랜치의 PR 이다.
 */

/** 상태 리액션 — 규약상 이 둘뿐. `THUMBS_UP` 은 옛 규약(수정 완료)의 잔재를 떼기 위해 제거 목록에만 있다. */
export const STATE_REACTIONS = ['EYES', 'ROCKET'] as const;
export type StateReaction = (typeof STATE_REACTIONS)[number];
const REMOVABLE = ['EYES', 'ROCKET', 'THUMBS_UP'] as const;

/** 재리뷰를 "왔다" 고 판정하는 봇 로그인. 사람 리뷰는 대기 종료 조건이 아니라 결과에만 실린다. */
export const REVIEW_BOTS = ['chatgpt-codex-connector', 'copilot-pull-request-reviewer'] as const;

export type ThreadNode = {
  id: string;
  isResolved: boolean;
  isOutdated: boolean;
  path: string;
  line: number | null;
  comments: {
    nodes: Array<{
      id: string;
      author: { login: string } | null;
      body: string;
      createdAt: string;
      reactions: { nodes: Array<{ content: string; user: { login: string } | null }> };
    }>;
  };
};

export type ThreadSummary = {
  threadId: string;
  /** 첫 코멘트 id — 리액션은 여기에 단다(스레드 id 가 아니다). */
  commentId: string;
  path: string;
  line: number | null;
  outdated: boolean;
  author: string;
  createdAt: string;
  /** 내 계정이 단 상태 리액션. 규약대로면 0 또는 1개. */
  mine: string[];
  body: string;
};

/** 미해결 스레드만 골라 첫 코멘트 기준으로 납작하게 편다. */
export function summarizeThreads(nodes: ThreadNode[], me: string): ThreadSummary[] {
  const out: ThreadSummary[] = [];
  for (const node of nodes) {
    if (node.isResolved) {
      continue;
    }
    const first = node.comments.nodes[0];
    if (!first) {
      continue;
    }
    out.push({
      threadId: node.id,
      commentId: first.id,
      path: node.path,
      line: node.line,
      outdated: node.isOutdated,
      author: first.author?.login ?? '(unknown)',
      createdAt: first.createdAt,
      mine: first.reactions.nodes
        .filter((r) => r.user?.login === me && (REMOVABLE as readonly string[]).includes(r.content))
        .map((r) => r.content),
      body: first.body,
    });
  }
  return out;
}

/** `target` 을 달기 전에 떼야 할 내 리액션 — 스레드당 하나 규약. */
export function reactionsToRemove(current: string[], target: StateReaction): string[] {
  return REMOVABLE.filter((c) => c !== target && current.includes(c));
}

export type ReviewNode = { author: { login: string } | null; submittedAt: string | null };
/** PR 본문에 달린 리액션 — Codex 는 지적이 없으면 코멘트 대신 여기에 👍 만 남긴다. */
export type ReactionNode = { content: string; createdAt: string; user: { login: string } | null };

/** 봇 리뷰 판정. `pending` = 아직(또는 안) 봄, `findings` = 리뷰 코멘트 제출, `clean` = 👍 만. */
export type BotVerdict = 'pending' | 'findings' | 'clean';

/**
 * 현재 head 커밋 이후의 봇 신호를 읽는다 — 재리뷰 대기의 종료 조건.
 *
 * Codex 는 두 가지 방식으로 끝을 알린다: 지적이 있으면 리뷰(코멘트)를 제출하고, 없으면
 * **PR 본문에 👍 리액션만** 단다(공식 문서: "automatically suggests improvements (or reacts
 * with 👍)"). 리액션은 웹훅도 check run 도 없어서 리뷰만 보면 깨끗한 PR 이 영원히 `pending` 이다.
 * 리뷰 중에는 👀 를 단다 — 그건 아직 `pending` 이다.
 */
export function botVerdict(
  reviews: ReviewNode[],
  reactions: ReactionNode[],
  headCommittedAt: string,
): BotVerdict {
  const head = Date.parse(headCommittedAt);
  // 리뷰 author 는 `chatgpt-codex-connector`, 본문 리액션의 user 는 `chatgpt-codex-connector[bot]` 로
  // 온다 — 같은 앱인데 GraphQL 이 자리마다 다르게 적는다. 접미사를 떼고 비교한다.
  const isBot = (login: string | undefined) =>
    login !== undefined &&
    (REVIEW_BOTS as readonly string[]).includes(login.replace(/\[bot\]$/, ''));
  const reviewed = reviews.some(
    (r) => isBot(r.author?.login) && r.submittedAt !== null && Date.parse(r.submittedAt) > head,
  );
  if (reviewed) {
    return 'findings';
  }
  const clean = reactions.some(
    (r) => isBot(r.user?.login) && r.content === 'THUMBS_UP' && Date.parse(r.createdAt) > head,
  );
  return clean ? 'clean' : 'pending';
}

export type Args = {
  cmd: 'list' | 'react' | 'watch';
  pr?: number;
  commentId?: string;
  reaction?: StateReaction;
  timeoutSec: number;
};

/** argv 해석. 잘못된 입력은 메시지와 함께 던진다 — 조용히 기본값으로 흘리지 않는다. */
export function parseArgs(argv: string[]): Args {
  const [cmd, ...rest] = argv;
  const args: Args = { cmd: 'list', timeoutSec: 300 };
  const positional: string[] = [];
  for (let i = 0; i < rest.length; i++) {
    const a = rest[i] ?? '';
    if (a === '--timeout') {
      const raw = rest[++i];
      const v = Number(raw);
      if (raw === undefined || !Number.isFinite(v) || v <= 0) {
        throw new Error(`--timeout 은 양수 초여야 한다 — 받은 값: ${raw ?? '(없음)'}`);
      }
      args.timeoutSec = v;
    } else if (a.startsWith('--')) {
      throw new Error(`모르는 옵션: ${a}`);
    } else {
      positional.push(a);
    }
  }
  switch (cmd) {
    case 'list':
    case 'watch': {
      args.cmd = cmd;
      if (positional[0] !== undefined) {
        args.pr = parsePrNumber(positional[0]);
      }
      return args;
    }
    case 'react': {
      args.cmd = cmd;
      const [commentId, reaction] = positional;
      if (!commentId || !reaction) {
        throw new Error('react <COMMENT_ID> <EYES|ROCKET> — 둘 다 필요하다');
      }
      if (!(STATE_REACTIONS as readonly string[]).includes(reaction)) {
        throw new Error(`상태 리액션은 ${STATE_REACTIONS.join('|')} 뿐이다 — 받은 값: ${reaction}`);
      }
      args.commentId = commentId;
      args.reaction = reaction as StateReaction;
      return args;
    }
    default:
      throw new Error(`사용법: pr-threads.ts <list|react|watch> … (받은 명령: ${cmd ?? '(없음)'})`);
  }
}

function parsePrNumber(raw: string): number {
  const n = Number(raw);
  if (!Number.isInteger(n) || n <= 0) {
    throw new Error(`PR 번호가 아니다: ${raw}`);
  }
  return n;
}

// ── gh 실행 ──────────────────────────────────────────────────────────────────

function gh(args: string[]): string {
  const result = Bun.spawnSync(['gh', ...args], { stdout: 'pipe', stderr: 'pipe' });
  if (result.exitCode !== 0) {
    throw new Error(
      `gh ${args.slice(0, 2).join(' ')} 실패(exit ${result.exitCode}): ${result.stderr.toString().trim()}`,
    );
  }
  return result.stdout.toString();
}

function graphql<T>(query: string, vars: Record<string, string | number>): T {
  const args = ['api', 'graphql', '-f', `query=${query}`];
  for (const [k, v] of Object.entries(vars)) {
    args.push(typeof v === 'number' ? '-F' : '-f', `${k}=${v}`);
  }
  const parsed = JSON.parse(gh(args)) as { data?: T; errors?: Array<{ message: string }> };
  if (parsed.errors?.length) {
    throw new Error(`GraphQL 에러: ${parsed.errors.map((e) => e.message).join('; ')}`);
  }
  if (!parsed.data) {
    throw new Error('GraphQL 응답에 data 가 없다');
  }
  return parsed.data;
}

type Slug = { owner: string; repo: string };

function repoSlug(): Slug {
  const json = JSON.parse(gh(['repo', 'view', '--json', 'owner,name'])) as {
    owner: { login: string };
    name: string;
  };
  return { owner: json.owner.login, repo: json.name };
}

function currentPr(): number {
  const json = JSON.parse(gh(['pr', 'view', '--json', 'number'])) as { number: number };
  return json.number;
}

const PR_QUERY = `
query($owner:String!, $repo:String!, $num:Int!, $after:String) {
  viewer { login }
  repository(owner:$owner, name:$repo) { pullRequest(number:$num) {
    headRefOid mergeStateStatus isDraft
    commits(last:1){ nodes{ commit{ committedDate } } }
    reviews(last:30){ nodes{ author{ login } submittedAt } }
    reactions(first:30){ nodes{ content createdAt user{ login } } }
    reviewThreads(first:100, after:$after){
      pageInfo{ hasNextPage endCursor }
      nodes{
        id isResolved isOutdated path line
        comments(first:1){ nodes{ id author{ login } body createdAt
          reactions(first:30){ nodes{ content user{ login } } } } }
      }
    }
  }}
}`;

type PrData = {
  viewer: { login: string };
  repository: {
    pullRequest: {
      headRefOid: string;
      mergeStateStatus: string;
      isDraft: boolean;
      commits: { nodes: Array<{ commit: { committedDate: string } }> };
      reviews: { nodes: ReviewNode[] };
      reactions: { nodes: ReactionNode[] };
      reviewThreads: {
        pageInfo: { hasNextPage: boolean; endCursor: string | null };
        nodes: ThreadNode[];
      };
    };
  };
};

export type PrSnapshot = {
  pr: number;
  head: string;
  headCommittedAt: string;
  mergeState: string;
  isDraft: boolean;
  me: string;
  reviews: ReviewNode[];
  reactions: ReactionNode[];
  threads: ThreadSummary[];
};

/** 스레드 전 페이지를 모아 스냅샷 하나로. */
function snapshot(slug: Slug, pr: number): PrSnapshot {
  const nodes: ThreadNode[] = [];
  let after: string | null = null;
  let first: PrData | undefined;
  do {
    const vars: Record<string, string | number> = { owner: slug.owner, repo: slug.repo, num: pr };
    if (after) {
      vars.after = after;
    }
    const data: PrData = graphql<PrData>(PR_QUERY, vars);
    first ??= data;
    const page = data.repository.pullRequest.reviewThreads;
    nodes.push(...page.nodes);
    after = page.pageInfo.hasNextPage ? page.pageInfo.endCursor : null;
  } while (after);
  if (!first) {
    throw new Error(`PR #${pr} 조회 결과가 비었다`);
  }
  const p = first.repository.pullRequest;
  return {
    pr,
    head: p.headRefOid.slice(0, 7),
    headCommittedAt: p.commits.nodes[0]?.commit.committedDate ?? '',
    mergeState: p.mergeStateStatus,
    isDraft: p.isDraft,
    me: first.viewer.login,
    reviews: p.reviews.nodes,
    reactions: p.reactions.nodes,
    threads: summarizeThreads(nodes, first.viewer.login),
  };
}

function react(commentId: string, target: StateReaction): { removed: string[]; added: string } {
  // 내 리액션 목록을 먼저 본다 — 없는 리액션을 removeReaction 하면 에러라 맹목 제거는 시끄럽다.
  const data = graphql<{
    viewer: { login: string };
    node: {
      reactions: { nodes: Array<{ content: string; user: { login: string } | null }> };
    } | null;
  }>(
    `query($id:ID!){ viewer{ login } node(id:$id){ ... on PullRequestReviewComment{ reactions(first:30){ nodes{ content user{ login } } } } } }`,
    { id: commentId },
  );
  if (!data.node) {
    throw new Error(`리뷰 코멘트가 아니거나 없다: ${commentId}`);
  }
  const mine = data.node.reactions.nodes
    .filter((r) => r.user?.login === data.viewer.login)
    .map((r) => r.content);
  const removed = reactionsToRemove(mine, target);
  for (const content of removed) {
    graphql(
      `mutation($id:ID!, $content:ReactionContent!){ removeReaction(input:{subjectId:$id, content:$content}){ reaction{ content } } }`,
      { id: commentId, content },
    );
  }
  if (!mine.includes(target)) {
    graphql(
      `mutation($id:ID!, $content:ReactionContent!){ addReaction(input:{subjectId:$id, content:$content}){ reaction{ content } } }`,
      { id: commentId, content: target },
    );
  }
  return { removed, added: target };
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/** CI 종료 → 봇 재리뷰(현재 head 이후 제출) 또는 timeout 까지 대기. */
async function watch(
  slug: Slug,
  pr: number,
  timeoutSec: number,
): Promise<PrSnapshot & { ci: 'pass' | 'fail'; verdict: BotVerdict }> {
  const checks = Bun.spawnSync(['gh', 'pr', 'checks', String(pr), '--watch'], {
    stdout: 'ignore',
    stderr: 'pipe',
  });
  const ci = checks.exitCode === 0 ? 'pass' : 'fail';
  const deadline = Date.now() + timeoutSec * 1000;
  let snap = snapshot(slug, pr);
  let verdict = botVerdict(snap.reviews, snap.reactions, snap.headCommittedAt);
  while (verdict === 'pending' && Date.now() < deadline) {
    await sleep(10_000);
    snap = snapshot(slug, pr);
    verdict = botVerdict(snap.reviews, snap.reactions, snap.headCommittedAt);
  }
  return { ...snap, ci, verdict };
}

async function main(argv: string[]): Promise<number> {
  let args: Args;
  try {
    args = parseArgs(argv);
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    return 2;
  }
  try {
    if (args.cmd === 'react') {
      const result = react(args.commentId as string, args.reaction as StateReaction);
      console.log(JSON.stringify(result));
      return 0;
    }
    const slug = repoSlug();
    const pr = args.pr ?? currentPr();
    const result =
      args.cmd === 'watch' ? await watch(slug, pr, args.timeoutSec) : snapshot(slug, pr);
    const { reviews: _reviews, reactions: _reactions, ...printable } = result;
    console.log(JSON.stringify(printable, null, 2));
    return 0;
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    return 1;
  }
}

if (import.meta.main) {
  process.exit(await main(process.argv.slice(2)));
}
