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
 *   # CI 가 끝날 때까지 → 현재 head 의 봇 신호(리뷰 코멘트 또는 본문 👍)가 올 때까지(또는 timeout) 기다린 뒤 list 출력
 *   # verdict: findings(리뷰 제출) / clean(👍 만 — 지적 없음) / pending(안 옴) / none(이 레포엔 리뷰 봇이 없다 — 기다리지 않는다)
 * bun scripts/pr-threads.ts ready 154           # "확인·머지해도 되나" 한 번에 — CI 초록 + 👀 없는 스레드 없음 + 🚀 없음. exit 0/1
 * bun scripts/pr-threads.ts transitions --interval 60
 *   # 열린 PR 전체를 돌며 MERGED / CLOSED / DIRTY / CONFLICTING 전이만 한 줄씩 — Monitor 에 물린다
 *   # (폴백 — repo 가 설정된 보드의 PR 은 데몬 `rockyd::prwatch` 가 보고 알린다)
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

export type ReviewNode = {
  author: { login: string } | null;
  submittedAt: string | null;
  /** 리뷰가 붙은 커밋 — Codex 의 "Reviewed commit:" 와 같은 값. 있으면 시각 대신 이걸로 head 와 대조한다. */
  commit?: { oid: string } | null;
};
/** PR 본문에 달린 리액션 — Codex 는 지적이 없으면 코멘트 대신 여기에 👍 만 남긴다. */
export type ReactionNode = { content: string; createdAt: string; user: { login: string } | null };

/**
 * 봇 리뷰 판정. `pending` = 아직(또는 안) 봄, `findings` = 리뷰 코멘트 제출, `clean` = 👍 만,
 * `none` = 이 레포에 리뷰 봇이 붙어 있지 않다(최근 PR 어디에도 봇 흔적이 없다 — 기다려도 안 온다).
 */
export type BotVerdict = 'pending' | 'findings' | 'clean' | 'none';

/**
 * 리뷰 봇 로그인인가. 리뷰 author 는 `chatgpt-codex-connector`, 본문 리액션의 user 는
 * `chatgpt-codex-connector[bot]` 로 온다 — 같은 앱인데 GraphQL 이 자리마다 다르게 적는다.
 */
function isReviewBot(login: string | undefined): boolean {
  return (
    login !== undefined &&
    (REVIEW_BOTS as readonly string[]).includes(login.replace(/\[bot\]$/, ''))
  );
}

/** 최근 PR 하나의 봇 흔적 재료 — 리뷰 작성자와 본문 리액션 작성자만. */
export type RecentPr = {
  reviews: { nodes: Array<{ author: { login: string } | null }> };
  reactions: { nodes: Array<{ user: { login: string } | null }> };
};

/**
 * 이 레포에 리뷰 봇이 붙어 있는가 — 최근 PR 중 하나라도 봇이 리뷰했거나 본문에 리액션(👀·👍)을
 * 달았으면. 봇 설치는 레포·계정마다 다르고(회사 레포엔 없다) 설정을 따로 두면 드리프트가 난다 —
 * 흔적으로 판단한다. 새로 봇을 붙인 레포는 첫 PR 에서 봇이 👀 를 다는 순간부터 잡힌다.
 */
export function hasReviewBot(prs: RecentPr[]): boolean {
  return prs.some(
    (p) =>
      p.reviews.nodes.some((r) => isReviewBot(r.author?.login)) ||
      p.reactions.nodes.some((r) => isReviewBot(r.user?.login)),
  );
}

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
  head?: string,
): BotVerdict {
  const headAt = Date.parse(headCommittedAt);
  const isBot = isReviewBot;
  // "이 head 를 봤는가" — 리뷰에 커밋이 실려 있으면 그것으로(Codex 의 "Reviewed commit:"),
  // 없으면 제출 시각이 head 커밋 시각보다 뒤인지로. 서버 리베이스로 head 가 바뀌면 시각 비교는
  // 옛 리뷰를 새것으로 오인할 수 있어 커밋 대조가 정확하다.
  const forHead = (r: ReviewNode) =>
    r.commit?.oid && head
      ? r.commit.oid.startsWith(head)
      : r.submittedAt !== null && Date.parse(r.submittedAt) > headAt;
  const reviewed = reviews.some((r) => isBot(r.author?.login) && forHead(r));
  if (reviewed) {
    return 'findings';
  }
  const clean = reactions.some(
    (r) => isBot(r.user?.login) && r.content === 'THUMBS_UP' && Date.parse(r.createdAt) > headAt,
  );
  return clean ? 'clean' : 'pending';
}

export type CiState = 'pass' | 'fail' | 'pending';

export type ReadyVerdict = {
  ready: boolean;
  ci: CiState;
  /** 미해결 스레드 수 / 그중 👀 도 🚀 도 없는 것 / 🚀(호출자 결정 필요). */
  threads: { total: number; unhandled: number; rocket: number };
  /** 안 되는 이유들 — 비어 있으면 ready. 사람이 읽을 문장. */
  reasons: string[];
};

/**
 * "확인·머지해도 되나" — `/rocky:resolve-reviews` 9단계의 세 조건을 한 번에. 스레드 수는
 * 판정 조건이 아니다(닫는 건 사용자 몫) — 처리 안 된 것과 결정 필요한 것만 막는다.
 */
export function readyVerdict(threads: ThreadSummary[], ci: CiState): ReadyVerdict {
  const rocket = threads.filter((t) => t.mine.includes('ROCKET')).length;
  const unhandled = threads.filter(
    (t) => !t.mine.includes('EYES') && !t.mine.includes('ROCKET'),
  ).length;
  const reasons: string[] = [];
  if (ci === 'fail') {
    reasons.push('CI 실패');
  } else if (ci === 'pending') {
    reasons.push('CI 진행 중');
  }
  if (unhandled > 0) {
    reasons.push(`처리 안 된 스레드 ${unhandled}건(👀 도 🚀 도 없음)`);
  }
  if (rocket > 0) {
    reasons.push(`호출자 결정 필요 🚀 ${rocket}건 — 알림이 아니라 질문이 먼저`);
  }
  return {
    ready: reasons.length === 0,
    ci,
    threads: { total: threads.length, unhandled, rocket },
    reasons,
  };
}

/** `gh pr checks` 한 줄의 둘째 칸(pass/fail/pending/skipping)을 모아 하나로. */
export function ciStateOf(rows: string[]): CiState {
  const states = rows.map((r) => r.split('\t')[1] ?? '');
  if (states.some((x) => x === 'fail')) {
    return 'fail';
  }
  if (states.some((x) => x === 'pending')) {
    return 'pending';
  }
  return 'pass';
}

export type PrState = { number: number; state: string; mergeState: string };

/**
 * 두 스냅숏 사이의 전이 중 사람이 움직여야 하는 것만 — 머지·닫힘·충돌. 새로 생긴 PR 이나
 * 리뷰 상태 변화는 내지 않는다(그건 다른 경로가 본다).
 */
export function transitionsBetween(prev: PrState[], cur: PrState[]): string[] {
  const before = new Map(prev.map((p) => [p.number, p]));
  const out: string[] = [];
  for (const p of cur) {
    const was = before.get(p.number);
    if (!was) {
      continue;
    }
    if (p.state !== was.state && (p.state === 'MERGED' || p.state === 'CLOSED')) {
      out.push(`#${p.number} ${p.state}`);
    } else if (
      p.mergeState !== was.mergeState &&
      (p.mergeState === 'DIRTY' || p.mergeState === 'CONFLICTING')
    ) {
      out.push(`#${p.number} ${p.state} ${p.mergeState}`);
    }
  }
  return out;
}

export type Args = {
  cmd: 'list' | 'react' | 'watch' | 'ready' | 'transitions';
  pr?: number;
  commentId?: string;
  reaction?: StateReaction;
  timeoutSec: number;
  intervalSec: number;
};

/** argv 해석. 잘못된 입력은 메시지와 함께 던진다 — 조용히 기본값으로 흘리지 않는다. */
export function parseArgs(argv: string[]): Args {
  const [cmd, ...rest] = argv;
  const args: Args = { cmd: 'list', timeoutSec: 300, intervalSec: 60 };
  const positional: string[] = [];
  for (let i = 0; i < rest.length; i++) {
    const a = rest[i] ?? '';
    if (a === '--timeout' || a === '--interval') {
      const raw = rest[++i];
      const v = Number(raw);
      if (raw === undefined || !Number.isFinite(v) || v <= 0) {
        throw new Error(`${a} 은 양수 초여야 한다 — 받은 값: ${raw ?? '(없음)'}`);
      }
      if (a === '--timeout') {
        args.timeoutSec = v;
      } else {
        args.intervalSec = v;
      }
    } else if (a.startsWith('--')) {
      throw new Error(`모르는 옵션: ${a}`);
    } else {
      positional.push(a);
    }
  }
  switch (cmd) {
    case 'transitions': {
      args.cmd = cmd;
      if (positional.length > 0) {
        throw new Error('transitions 는 PR 번호를 받지 않는다 — 열린 PR 전체를 본다');
      }
      return args;
    }
    case 'list':
    case 'ready':
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
      throw new Error(
        `사용법: pr-threads.ts <list|react|watch|ready|transitions> … (받은 명령: ${cmd ?? '(없음)'})`,
      );
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
    reviews(last:30){ nodes{ author{ login } submittedAt commit{ oid } } }
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

/** 이 PR 을 포함한 최근 PR 20개의 봇 흔적 — 요청 노드가 작아 비용이 싸다. */
const RECENT_BOT_QUERY = `query($owner:String!, $repo:String!){
  repository(owner:$owner, name:$repo){
    pullRequests(last:20, states:[OPEN, MERGED, CLOSED]){ nodes{
      reviews(first:10){ nodes{ author{ login } } }
      reactions(first:10){ nodes{ user{ login } } }
    } }
  }
}`;

function repoHasReviewBot(slug: Slug): boolean {
  const data = graphql<{ repository: { pullRequests: { nodes: RecentPr[] } } }>(RECENT_BOT_QUERY, {
    owner: slug.owner,
    repo: slug.repo,
  });
  return hasReviewBot(data.repository.pullRequests.nodes);
}

/**
 * CI 종료 → 봇 재리뷰(현재 head 이후 제출) 또는 timeout 까지 대기. 리뷰 봇이 없는 레포면
 * 기다리지 않고 `none` — 안 올 신호를 timeout 까지 기다리지 않는다.
 */
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
  let verdict = botVerdict(snap.reviews, snap.reactions, snap.headCommittedAt, snap.head);
  if (verdict === 'pending' && !repoHasReviewBot(slug)) {
    return { ...snap, ci, verdict: 'none' };
  }
  while (verdict === 'pending' && Date.now() < deadline) {
    await sleep(10_000);
    snap = snapshot(slug, pr);
    verdict = botVerdict(snap.reviews, snap.reactions, snap.headCommittedAt, snap.head);
  }
  return { ...snap, ci, verdict };
}

/** `gh pr checks` 를 기다리지 않고 지금 상태만 — pass/fail/pending. check 가 없으면 pass 로 본다. */
function ciNow(pr: number): CiState {
  const result = Bun.spawnSync(['gh', 'pr', 'checks', String(pr)], {
    stdout: 'pipe',
    stderr: 'pipe',
  });
  const rows = result.stdout.toString().trim().split('\n').filter(Boolean);
  if (rows.length === 0) {
    return 'pass';
  }
  return ciStateOf(rows);
}

function ready(slug: Slug, pr: number): PrSnapshot & { verdict: ReadyVerdict } {
  const snap = snapshot(slug, pr);
  return { ...snap, verdict: readyVerdict(snap.threads, ciNow(pr)) };
}

function openPrStates(): PrState[] {
  const raw = gh([
    'pr',
    'list',
    '--state',
    'all',
    '--limit',
    '20',
    '--json',
    'number,state,mergeStateStatus',
  ]);
  const rows = JSON.parse(raw) as Array<{
    number: number;
    state: string;
    mergeStateStatus: string;
  }>;
  return rows.map((r) => ({ number: r.number, state: r.state, mergeState: r.mergeStateStatus }));
}

/** 끝나지 않는다 — Monitor 가 죽일 때까지 전이만 한 줄씩. 한 번의 조회 실패는 건너뛴다. */
async function transitions(intervalSec: number): Promise<never> {
  let prev: PrState[] | null = null;
  for (;;) {
    try {
      const cur = openPrStates();
      if (prev) {
        for (const line of transitionsBetween(prev, cur)) {
          console.log(line);
        }
      }
      prev = cur;
    } catch {
      // 일시 실패 — 다음 tick 에.
    }
    await sleep(intervalSec * 1000);
  }
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
    if (args.cmd === 'transitions') {
      await transitions(args.intervalSec);
    }
    const slug = repoSlug();
    const pr = args.pr ?? currentPr();
    if (args.cmd === 'ready') {
      const result = ready(slug, pr);
      const { reviews: _reviews, reactions: _reactions, ...printable } = result;
      console.log(JSON.stringify(printable, null, 2));
      return result.verdict.ready ? 0 : 1;
    }
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
