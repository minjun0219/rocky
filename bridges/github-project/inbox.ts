#!/usr/bin/env bun
/**
 * GitHub 프로젝트 보드 수집함 — `rocky.json` 의 `todo.inbox[]` 에 등록한다.
 *
 * ```json
 * { "todo": { "inbox": [
 *   { "name": "gh-bugs", "command": ["bun", "/path/to/rocky/bridges/github-project/inbox.ts",
 *       "--project", "OWNER/NUMBER", "--assignee", "@me", "--type", "Bug", "--field", "Component/s=Web"],
 *     "timeoutMs": 20000 }
 * ] } }
 * ```
 *
 * 프로젝트 보드의 필터(`assignee:@me type:Bug component/s:Web`)를 인자로 옮긴 것이다. 보드를 훑지 않고
 * **이슈 검색**(`user:OWNER is:issue is:open` + 담당자·타입)으로 먼저 좁힌 뒤, 이슈마다 이 보드의 항목만
 * 골라 필드를 본다 — 보드가 커도 비용과 결과가 같다. 조건은 전부 AND, 대소문자 무시:
 *
 * - `--assignee LOGIN|@me` — 담당자 중에 있어야 한다(`@me` 는 `gh` 로그인 계정).
 * - `--type NAME` — 이슈 타입(`type:`). 보드 주인이 **개인 계정**이면 이슈 타입이 없으므로 같은 이름의
 *   **라벨**(`label:`)로 대신한다.
 * - `--field "이름=값"` — 보드의 사용자 정의 필드(단일 선택·텍스트·반복·숫자). 여러 번 줄 수 있다.
 *
 * 인증은 로그인된 `gh` 를 그대로 쓴다(`read:project` 권한 필요). 토큰을 따로 읽지 않는다.
 * 범위는 **보드 주인이 가진 레포**의 이슈다 — 다른 주인의 레포에서 보드에 올린 이슈는 검색에 안 잡힌다.
 * 결과가 잘리면(검색 1,000건 초과·보드 5개 넘게 걸린 이슈·필드 50개 초과) 조용히 빼지 않고 실패한다.
 *
 * GraphQL 예산은 PR 감시와 같은 계정에 걸린다 — 데몬은 이 명령을 보드를 볼 때만 부르고 60초 캐시한다.
 * `--limit`(기본 100)은 검색 한 페이지 크기이고 결과는 끝까지 넘긴다. `--from FILE` 이면 저장된 응답(한
 * 페이지 또는 페이지 배열)을 변환한다(테스트).
 */

export type InboxItem = {
  id: string;
  title: string;
  url: string;
  note?: string;
  createdAt?: string;
};

export type Filters = {
  assignee?: string;
  type?: string;
  fields: { name: string; value: string }[];
};

type Args = Filters & { project?: string; limit: number; from?: string };

function fail(message: string): never {
  process.stderr.write(`github-project: ${message}\n`);
  process.exit(1);
}

export function parseArgs(argv: string[]): Args {
  const out: Args = { fields: [], limit: 100 };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    const value = argv[i + 1];
    if (value === undefined && arg.startsWith('--')) {
      fail(`${arg} 에 값이 없다`);
    }
    if (arg === '--project') {
      out.project = value;
    } else if (arg === '--assignee') {
      out.assignee = value;
    } else if (arg === '--type') {
      out.type = value;
    } else if (arg === '--field') {
      const at = value.indexOf('=');
      if (at <= 0) {
        fail(`--field 는 "이름=값" 이어야 한다: ${value}`);
      }
      out.fields.push({ name: value.slice(0, at).trim(), value: value.slice(at + 1).trim() });
    } else if (arg === '--limit') {
      const n = Number(value);
      if (!Number.isInteger(n) || n < 1 || n > 100) {
        fail(`--limit 는 1~100: ${value}`);
      }
      out.limit = n;
    } else if (arg === '--from') {
      out.from = value;
    } else {
      fail(`모르는 인자: ${arg}`);
    }
    i += 1;
  }
  return out;
}

/** `OWNER/NUMBER` → 둘로. 사용자·조직 어느 쪽 보드든 된다. */
export function parseProject(raw: string): { owner: string; number: number } {
  const match = /^([A-Za-z0-9-]+)\/(\d+)$/.exec(raw.trim());
  if (!match) {
    fail(`--project 는 OWNER/NUMBER 여야 한다: ${raw}`);
  }
  return { owner: match[1], number: Number(match[2]) };
}

/**
 * 이슈 검색 한 페이지 — 보드를 훑지 않고 검색 한정자로 먼저 좁힌 뒤, 이슈마다 **그 보드의 항목**만 골라
 * `--field` 를 본다. 비용은 보드 크기와 무관하다: 요청 노드 기준 `first:100` × `projectItems(first:5)`
 * ≈ 6 포인트(실측). `projectItems` 를 20 으로 두면 21 이다 — 이슈 하나가 보드 5개를 넘게 걸리는 일은
 * 드물고, 넘쳐서 이 보드를 못 찾으면 조용히 빼지 않고 실패한다.
 */
export const QUERY = `query($owner: String!, $q: String!, $limit: Int!, $after: String) {
  repositoryOwner(login: $owner) { __typename }
  search(type: ISSUE, query: $q, first: $limit, after: $after) {
    issueCount
    pageInfo { hasNextPage endCursor }
    nodes { ... on Issue {
      number title url state createdAt
      repository { nameWithOwner }
      projectItems(first: 5) {
        pageInfo { hasNextPage }
        nodes {
          project { number owner { ... on Organization { login } ... on User { login } } }
          fieldValues(first: 50) {
            pageInfo { hasNextPage }
            nodes {
              ... on ProjectV2ItemFieldSingleSelectValue { name field { ... on ProjectV2FieldCommon { name } } }
              ... on ProjectV2ItemFieldTextValue { text field { ... on ProjectV2FieldCommon { name } } }
              ... on ProjectV2ItemFieldIterationValue { title field { ... on ProjectV2FieldCommon { name } } }
              ... on ProjectV2ItemFieldNumberValue { number field { ... on ProjectV2FieldCommon { name } } }
            }
          }
        }
      }
    } }
  }
}`;

/** GitHub 검색이 돌려주는 결과 수의 상한 — 이보다 많으면 페이지를 넘겨도 끝까지 못 본다. */
export const SEARCH_MAX = 1000;

/** 검색어 값 — 공백이 있으면 따옴표로 감싼다. 값 속 따옴표는 한정자를 깨뜨리므로 뺀다. */
function qualifierValue(raw: string): string {
  const value = raw.replaceAll('"', '').trim();
  return /\s/.test(value) ? `"${value}"` : value;
}

/**
 * 검색어. 보드 주인의 레포로 좁힌다(`user:` 는 조직에도 먹는다 — 실측). `--type` 은 조직이면 이슈 타입
 * (`type:`), 개인 계정이면 같은 이름의 라벨(`label:`) — 개인 레포에는 이슈 타입이 없다.
 */
export function searchQuery(owner: string, ownerIsUser: boolean, filters: Filters): string {
  const parts = [`user:${owner}`, 'is:issue', 'is:open'];
  if (filters.assignee) {
    parts.push(`assignee:${qualifierValue(filters.assignee)}`);
  }
  if (filters.type) {
    parts.push(`${ownerIsUser ? 'label' : 'type'}:${qualifierValue(filters.type)}`);
  }
  return parts.join(' ');
}

type FieldValue = {
  name?: string;
  text?: string;
  title?: string;
  number?: number;
  field?: { name?: string };
};

type ProjectItem = {
  project?: { number?: number; owner?: { login?: string } };
  fieldValues?: { pageInfo?: { hasNextPage?: boolean }; nodes?: (FieldValue | null)[] };
};

type Issue = {
  number: number;
  title: string;
  url: string;
  state: string;
  createdAt?: string;
  repository?: { nameWithOwner: string };
  projectItems?: { pageInfo?: { hasNextPage?: boolean }; nodes?: (ProjectItem | null)[] };
};

export type SearchPage = {
  data?: {
    repositoryOwner?: { __typename?: string } | null;
    search?: {
      issueCount?: number;
      pageInfo?: { hasNextPage?: boolean; endCursor?: string | null };
      nodes?: (Partial<Issue> | null)[];
    };
  };
};

const same = (a: string, b: string) =>
  a.localeCompare(b, undefined, { sensitivity: 'accent' }) === 0;

/** 필드 값 하나를 글자로 — 단일 선택은 옵션 이름, 반복은 제목, 숫자는 그대로. */
function fieldText(v: FieldValue): string | undefined {
  if (typeof v.name === 'string') {
    return v.name;
  }
  if (typeof v.text === 'string') {
    return v.text;
  }
  if (typeof v.title === 'string') {
    return v.title;
  }
  if (typeof v.number === 'number') {
    return String(v.number);
  }
  return undefined;
}

/** 검색 한 페이지의 모양을 확인한다 — 틀리면 실패. */
function searchOf(page: SearchPage) {
  const data = page?.data;
  if (!data) {
    fail('응답에 data 가 없다');
  }
  if (!data.repositoryOwner) {
    fail('보드 주인(사용자·조직)을 찾지 못했다');
  }
  const search = data.search;
  if (!search || !Array.isArray(search.nodes)) {
    fail('응답에 search 가 없다');
  }
  return search;
}

/**
 * 검색 결과 페이지들 → 규약 item. 이슈마다 **이 보드의 항목**만 골라 `--field` 를 AND 로 본다. 조건은
 * 대소문자 무시. 결과가 잘렸으면(검색 페이지 누락·보드 항목·필드 값 넘침) 성공으로 보고하지 않고 실패한다.
 */
export function toItems(
  pages: SearchPage[],
  filters: Filters,
  board: { owner: string; number: number },
): InboxItem[] {
  if (pages.length === 0) {
    fail('검색 응답이 없다');
  }
  const searches = pages.map(searchOf);
  const total = searches[0].issueCount ?? 0;
  const got = searches.reduce((n, s) => n + (s.nodes?.length ?? 0), 0);
  if (searches.at(-1)?.pageInfo?.hasNextPage || got < total) {
    fail(`검색 결과가 잘렸다 — ${total}건 중 ${got}건만 받았다`);
  }
  const items: InboxItem[] = [];
  for (const issue of searches.flatMap((s) => s.nodes ?? [])) {
    if (!issue || typeof issue.number !== 'number' || typeof issue.url !== 'string') {
      continue; // PR·권한 없는 항목
    }
    if (issue.state !== 'OPEN') {
      continue;
    }
    const ref = `${issue.repository?.nameWithOwner ?? '?'}#${issue.number}`;
    const entries = issue.projectItems?.nodes ?? [];
    const item = entries.find(
      (e) =>
        e?.project?.number === board.number &&
        typeof e.project.owner?.login === 'string' &&
        same(e.project.owner.login, board.owner),
    );
    if (!item) {
      if (issue.projectItems?.pageInfo?.hasNextPage) {
        fail(`${ref} 이 보드 ${entries.length}개 넘게 걸려 이 보드의 항목인지 판정하지 못했다`);
      }
      continue; // 이 보드에 없는 이슈
    }
    const values = item.fieldValues?.nodes ?? [];
    const valuesCut = item.fieldValues?.pageInfo?.hasNextPage === true;
    const fieldsOk = filters.fields.every(({ name, value }) => {
      const hit = values.find((v) => v?.field?.name && same(v.field.name, name));
      if (!hit && valuesCut) {
        fail(`${ref} 의 필드 값이 ${values.length}개를 넘어 "${name}" 을 판정하지 못했다`);
      }
      const text = hit ? fieldText(hit) : undefined;
      return text !== undefined && same(text, value);
    });
    if (!fieldsOk) {
      continue;
    }
    const repo = issue.repository?.nameWithOwner;
    items.push({
      id: issue.url,
      title: (issue.title ?? '').trim() || `#${issue.number}`,
      url: issue.url,
      ...(repo ? { note: `${repo}#${issue.number}` } : {}),
      ...(issue.createdAt ? { createdAt: issue.createdAt } : {}),
    });
  }
  // 새 이슈가 위로.
  items.sort((a, b) => (b.createdAt ?? '').localeCompare(a.createdAt ?? ''));
  return items;
}

function spawnGh(cmd: string[]) {
  return Bun.spawn(cmd, { stdout: 'pipe', stderr: 'pipe' });
}

async function graphql(vars: Record<string, string | number>): Promise<SearchPage> {
  const flags = Object.entries(vars).flatMap(([k, v]) =>
    typeof v === 'number' ? ['-F', `${k}=${v}`] : ['-f', `${k}=${v}`],
  );
  let proc: ReturnType<typeof Bun.spawn>;
  try {
    proc = spawnGh(['gh', 'api', 'graphql', '-f', `query=${QUERY}`, ...flags]);
  } catch (error) {
    // gh 가 없거나 실행할 수 없으면 spawn 이 던진다 — 데몬은 stderr 첫 줄을 사유로 보여 주므로 한 줄로.
    const reason = (error instanceof Error ? error.message : String(error)).split('\n')[0];
    fail(`gh 를 실행하지 못했다(설치·PATH 확인): ${reason}`);
  }
  const timer = setTimeout(() => proc.kill(), 15000);
  const [code, stdout, stderr] = await Promise.all([
    proc.exited,
    new Response(proc.stdout).text(),
    new Response(proc.stderr).text(),
  ]);
  clearTimeout(timer);
  if (code !== 0) {
    const reason = stderr.trim().split('\n')[0] || `exit ${code}`;
    fail(`gh api graphql 실패: ${reason}`);
  }
  try {
    return JSON.parse(stdout) as SearchPage;
  } catch {
    fail('gh 응답이 JSON 이 아니다');
  }
}

/**
 * 검색을 끝까지 넘긴다. 첫 호출은 조직이라 보고 `type:` 으로 묻는다 — 주인이 개인 계정이면(`__typename`)
 * `label:` 로 한 번 더 묻는다. 그래서 조직 보드는 결과가 한 페이지면 호출 1회로 끝난다.
 */
async function fetchAll(owner: string, filters: Filters, limit: number): Promise<SearchPage[]> {
  const run = async (ownerIsUser: boolean): Promise<SearchPage[]> => {
    const q = searchQuery(owner, ownerIsUser, filters);
    const pages = [await graphql({ owner, q, limit })];
    const total = pages[0].data?.search?.issueCount ?? 0;
    if (total > SEARCH_MAX) {
      fail(
        `검색 결과가 ${total}건 — GitHub 검색은 ${SEARCH_MAX}건까지라 끝까지 볼 수 없다. 조건을 좁혀라`,
      );
    }
    for (;;) {
      const info = pages.at(-1)?.data?.search?.pageInfo;
      if (!info?.hasNextPage || !info.endCursor) {
        return pages;
      }
      pages.push(await graphql({ owner, q, limit, after: info.endCursor }));
    }
  };
  const first = await run(false);
  const isUser = first[0].data?.repositoryOwner?.__typename === 'User';
  return isUser && filters.type ? run(true) : first;
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  if (!args.project) {
    fail('--project OWNER/NUMBER 가 필요하다');
  }
  const board = parseProject(args.project);
  let pages: SearchPage[];
  if (args.from) {
    // 테스트 — 저장된 응답 한 페이지, 또는 페이지 배열.
    const saved = JSON.parse(await Bun.file(args.from).text()) as SearchPage | SearchPage[];
    pages = Array.isArray(saved) ? saved : [saved];
  } else {
    pages = await fetchAll(board.owner, args, args.limit);
  }
  process.stdout.write(`${JSON.stringify({ items: toItems(pages, args, board) })}\n`);
}

if (import.meta.main) {
  await main();
}
