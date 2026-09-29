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
 * 프로젝트 보드의 필터(`assignee:@me type:Bug component/s:Web`)를 인자로 옮긴 것이다. 보드의 항목을
 * `gh api graphql` 로 받아 **열린 이슈만** 조건으로 거른다 — 조건은 전부 AND, 대소문자 무시:
 *
 * - `--assignee LOGIN|@me` — 담당자 중에 있어야 한다(`@me` 는 `gh` 로그인 계정).
 * - `--type NAME` — 이슈 타입. 조직 레포는 이슈 타입만 본다(타입 미지정이면 제외 — Projects 의 `type:`
 *   필터와 같다). 이슈 타입이 없는 **개인 계정 레포**만 같은 이름의 **라벨**로 대신한다.
 * - `--field "이름=값"` — 보드의 사용자 정의 필드(단일 선택·텍스트·반복·숫자). 여러 번 줄 수 있다.
 *
 * 인증은 로그인된 `gh` 를 그대로 쓴다(`read:project` 권한 필요). 토큰을 따로 읽지 않는다.
 * GraphQL 예산은 PR 감시와 같은 계정에 걸린다 — 데몬은 이 명령을 보드를 볼 때만 부르고 60초
 * 캐시하므로, 여기서는 `--limit`(기본 100) 만큼만 요청한다. `--from FILE` 이면 저장된 응답을 변환한다(테스트).
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

/** 사용자·조직 보드를 한 쿼리로 — 어느 쪽인지 모르니 둘 다 묻고 있는 쪽을 쓴다. */
export const QUERY = `query($owner: String!, $number: Int!, $limit: Int!) {
  viewer { login }
  repositoryOwner(login: $owner) {
    ... on User { projectV2(number: $number) { ...board } }
    ... on Organization { projectV2(number: $number) { ...board } }
  }
}
fragment board on ProjectV2 {
  title
  items(first: $limit) { nodes {
    content { ... on Issue {
      number title url state createdAt
      repository { nameWithOwner owner { __typename } }
      issueType { name }
      assignees(first: 10) { nodes { login } }
      labels(first: 50) { nodes { name } }
    } }
    fieldValues(first: 50) { nodes {
      ... on ProjectV2ItemFieldSingleSelectValue { name field { ... on ProjectV2FieldCommon { name } } }
      ... on ProjectV2ItemFieldTextValue { text field { ... on ProjectV2FieldCommon { name } } }
      ... on ProjectV2ItemFieldIterationValue { title field { ... on ProjectV2FieldCommon { name } } }
      ... on ProjectV2ItemFieldNumberValue { number field { ... on ProjectV2FieldCommon { name } } }
    } }
  } }
}`;

type Issue = {
  number: number;
  title: string;
  url: string;
  state: string;
  createdAt?: string;
  repository?: { nameWithOwner: string; owner?: { __typename?: string } };
  issueType?: { name: string } | null;
  assignees?: { nodes: { login: string }[] };
  labels?: { nodes: { name: string }[] };
};

type FieldValue = {
  name?: string;
  text?: string;
  title?: string;
  number?: number;
  field?: { name?: string };
};

type Node = { content?: Partial<Issue> | null; fieldValues?: { nodes: (FieldValue | null)[] } };

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

/** 응답 → 규약 item. 이슈가 아닌 항목(초안·PR)과 닫힌 이슈는 뺀다. 모양이 틀리면 실패. */
export function toItems(response: unknown, filters: Filters): InboxItem[] {
  const data = (response as { data?: Record<string, unknown> })?.data;
  if (!data) {
    fail('응답에 data 가 없다');
  }
  const viewer = (data.viewer as { login?: string } | undefined)?.login;
  const owner = data.repositoryOwner as { projectV2?: { items?: { nodes?: Node[] } } } | null;
  if (!owner) {
    fail('보드 주인(사용자·조직)을 찾지 못했다');
  }
  if (!owner.projectV2) {
    fail('보드를 찾지 못했다 — 번호, 또는 gh 의 read:project 권한');
  }
  const nodes = owner.projectV2.items?.nodes;
  if (!Array.isArray(nodes)) {
    fail('응답에 items 가 없다');
  }
  const wantAssignee =
    filters.assignee === '@me' ? viewer : filters.assignee?.replace(/^@/, '') || undefined;
  if (filters.assignee === '@me' && !viewer) {
    fail('@me 를 풀 viewer 가 응답에 없다');
  }
  const items: InboxItem[] = [];
  for (const node of nodes) {
    const issue = node?.content;
    if (!issue || typeof issue.number !== 'number' || typeof issue.url !== 'string') {
      continue; // 초안·PR·권한 없는 항목
    }
    if (issue.state !== 'OPEN') {
      continue;
    }
    if (wantAssignee) {
      const logins = issue.assignees?.nodes.map((a) => a.login) ?? [];
      if (!logins.some((l) => same(l, wantAssignee))) {
        continue;
      }
    }
    if (filters.type) {
      const type = issue.issueType?.name;
      // 라벨 대체는 이슈 타입 기능이 없는 개인 계정 레포에만 — 조직 레포의 타입 미지정 이슈는 제외다.
      const personal = issue.repository?.owner?.__typename === 'User';
      const ok = type
        ? same(type, filters.type)
        : personal && (issue.labels?.nodes ?? []).some((l) => same(l.name, filters.type as string));
      if (!ok) {
        continue;
      }
    }
    const values = node.fieldValues?.nodes ?? [];
    const fieldsOk = filters.fields.every(({ name, value }) =>
      values.some((v) => {
        if (!v?.field?.name || !same(v.field.name, name)) {
          return false;
        }
        const text = fieldText(v);
        return text !== undefined && same(text, value);
      }),
    );
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

async function fetchBoard(owner: string, number: number, limit: number): Promise<unknown> {
  let proc: ReturnType<typeof Bun.spawn>;
  try {
    proc = spawnGh([
      'gh',
      'api',
      'graphql',
      '-f',
      `query=${QUERY}`,
      '-F',
      `owner=${owner}`,
      '-F',
      `number=${number}`,
      '-F',
      `limit=${limit}`,
    ]);
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
    return JSON.parse(stdout);
  } catch {
    fail('gh 응답이 JSON 이 아니다');
  }
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  let response: unknown;
  if (args.from) {
    response = JSON.parse(await Bun.file(args.from).text());
  } else {
    if (!args.project) {
      fail('--project OWNER/NUMBER 가 필요하다');
    }
    const { owner, number } = parseProject(args.project);
    response = await fetchBoard(owner, number, args.limit);
  }
  process.stdout.write(`${JSON.stringify({ items: toItems(response, args) })}\n`);
}

if (import.meta.main) {
  await main();
}
