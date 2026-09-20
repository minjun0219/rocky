/**
 * 포인터(`경로:심볼` / `경로:42` / `경로:42-58`)를 GitHub permalink 로 바꾼다.
 *
 * `/rocky:finish` 의 PR 본문 "봐 주세요" 절이 쓴다. 매번 에이전트가 remote URL 을 파싱하고
 * 심볼의 줄 번호를 세는 대신 이 스크립트를 부른다 — 링크를 손으로 조립하면 owner/repo 를
 * 틀리거나 브랜치명으로 걸어(머지 후 깨진다) 조용히 죽은 링크가 남는다.
 *
 * ```
 * bun scripts/permalink.ts --pr 127 commands/finish.md:12-18
 * # → [commands/finish.md:12-18](https://github.com/…/pull/127/files#diff-<해시>R12-R18)
 *
 * bun scripts/permalink.ts src/core/handlers.ts:handleOpenapiSearch
 * # → [src/core/handlers.ts:118](https://github.com/…/blob/<sha>/src/core/handlers.ts#L118)
 * ```
 */

export type RepoSlug = { owner: string; repo: string };

/** 포인터 한 건. `line` 이 없으면 파일 전체를 가리킨다. */
export type Pointer = {
  path: string;
  /** 명시된 줄 범위. `symbol` 과 동시에 존재하지 않는다. */
  line?: { start: number; end: number };
  /** 줄 번호 대신 찾아야 할 심볼/문구. */
  symbol?: string;
};

const GITHUB_REMOTE =
  /^(?:(?:https?|ssh|git):\/\/)?(?:[^@/]+@)?github\.com[:/](?<owner>[^/]+)\/(?<repo>[^/]+?)(?:\.git)?\/?$/;

/**
 * `git remote get-url` 결과에서 owner/repo 를 뽑는다. SSH(`git@github.com:o/r.git`) 와
 * HTTPS(`https://github.com/o/r`) 를 모두 받는다.
 *
 * @throws GitHub 이 아닌 remote — permalink 형식이 다르므로 조용히 틀린 URL 을 만들지 않는다.
 */
export function parseRepoSlug(remoteUrl: string): RepoSlug {
  const match = GITHUB_REMOTE.exec(remoteUrl.trim());
  const owner = match?.groups?.owner;
  const repo = match?.groups?.repo;
  if (!owner || !repo) {
    throw new Error(
      `GitHub remote 가 아니다 — remote=${remoteUrl.trim() || '(비어 있음)'}. ` +
        'permalink 는 github.com 저장소에만 만든다.',
    );
  }
  return { owner, repo };
}

/**
 * 포인터 경로를 저장소 루트 기준 상대 경로로 정규화한다.
 *
 * 두 가지를 동시에 막는다 — `./commands/finish.md` 처럼 군더더기가 붙으면 `buildDiffLink` 의
 * sha256 앵커가 GitHub 의 실제 앵커(루트 기준 경로)와 어긋나 링크가 조용히 깨지고, `../` 로
 * 루트를 벗어나는 경로는 저장소 밖 파일을 읽게 한다.
 *
 * @throws 절대 경로이거나 `..` 가 루트를 벗어나는 경우, 정규화 결과가 빈 경로인 경우.
 */
function normalizePointerPath(path: string, raw: string): string {
  if (path.startsWith('/')) {
    throw new Error(`포인터 경로는 저장소 루트 기준 상대 경로여야 한다 — 입력=${raw}`);
  }

  const segments: string[] = [];
  for (const segment of path.split('/')) {
    if (segment === '' || segment === '.') {
      continue;
    }
    if (segment !== '..') {
      segments.push(segment);
      continue;
    }
    if (segments.length === 0) {
      throw new Error(`포인터가 저장소 밖을 가리킨다 — 입력=${raw}`);
    }
    segments.pop();
  }

  if (segments.length === 0) {
    throw new Error(`포인터에 경로가 없다 — 입력=${raw}`);
  }
  return segments.join('/');
}

/**
 * `경로:심볼` / `경로:42` / `경로:42-58` / `경로` 를 파싱한다. 경로는 저장소 루트 기준으로
 * 정규화된다 — 이후 단계(해시 앵커, 파일 읽기, 라벨)는 모두 이 값을 쓴다.
 *
 * @throws 경로가 비었거나 루트를 벗어나거나 줄 범위가 뒤집힌 경우.
 */
export function parsePointer(raw: string): Pointer {
  const trimmed = raw.trim();
  const cut = trimmed.lastIndexOf(':');
  const rawPath = cut === -1 ? trimmed : trimmed.slice(0, cut);
  const suffix = cut === -1 ? '' : trimmed.slice(cut + 1);

  if (!rawPath) {
    throw new Error(`포인터에 경로가 없다 — 입력=${raw}`);
  }
  const path = normalizePointerPath(rawPath, raw);
  if (!suffix) {
    return { path };
  }

  const range = /^(\d+)(?:-(\d+))?$/.exec(suffix);
  if (!range) {
    return { path, symbol: suffix };
  }

  const start = Number(range[1]);
  const end = range[2] ? Number(range[2]) : start;
  if (start < 1 || end < start) {
    throw new Error(`줄 범위가 잘못됐다 — 입력=${raw} (start=${start}, end=${end})`);
  }
  return { path, line: { start, end } };
}

// 심볼이 "정의"된 줄로 보이는 패턴 — 코드의 선언부와 마크다운 제목/번호 항목.
const DEFINITION_HINT =
  /^\s*(?:export\s+|declare\s+|public\s+|private\s+|async\s+)*(?:function|class|const|let|var|type|interface|enum|def|#{1,6}\s|\d+\.\s|-\s+\*\*)/;

/**
 * 파일 내용에서 심볼이 있는 줄 번호(1-based)를 찾는다. 정의처럼 보이는 줄을 우선한다.
 *
 * @throws 못 찾았거나 후보가 여럿일 때 — 링크를 임의로 하나 고르지 않고 후보를 보여준다.
 *   호출자는 줄 번호를 직접 지정해 다시 부르면 된다.
 */
export function resolveSymbolLine(content: string, symbol: string, path = ''): number {
  const lines = content.split('\n');
  const hits: number[] = [];
  for (const [index, line] of lines.entries()) {
    if (line.includes(symbol)) {
      hits.push(index + 1);
    }
  }

  if (hits.length === 0) {
    throw new Error(`심볼을 찾지 못했다 — ${path || '파일'} 안에 "${symbol}" 가 없다`);
  }

  const defs = hits.filter((n) => DEFINITION_HINT.test(lines[n - 1] ?? ''));
  const candidates = defs.length > 0 ? defs : hits;
  const [first] = candidates;
  if (candidates.length === 1 && first !== undefined) {
    return first;
  }

  const shown = candidates
    .slice(0, 10)
    .map((n) => `  ${n}: ${(lines[n - 1] ?? '').trim().slice(0, 80)}`)
    .join('\n');
  const more = candidates.length > 10 ? `\n  … 외 ${candidates.length - 10}건` : '';
  throw new Error(
    `"${symbol}" 후보가 ${candidates.length}건이라 하나로 정할 수 없다 — ${path}\n${shown}${more}\n` +
      `줄 번호를 직접 지정해 다시 부른다 (예: ${path || '<경로>'}:${first}).`,
  );
}

/** 링크 텍스트로 쓸 `경로:줄` 라벨. 줄이 없으면 경로만. */
export function formatPointerLabel(path: string, line?: { start: number; end: number }): string {
  if (!line) {
    return path;
  }
  return line.end > line.start ? `${path}:${line.start}-${line.end}` : `${path}:${line.start}`;
}

/**
 * PR 의 Files changed 안 해당 위치로 가는 URL 을 만든다.
 *
 * 앵커는 `#diff-<sha256(저장소 루트 기준 경로)>` + 오른쪽(변경 후) 줄 `R<n>` 이다. 파일 앵커가
 * 경로의 sha256 이라는 것은 실제 PR 의 Files changed HTML 로 확인했다.
 *
 * blob permalink 와 달리 **PR 번호가 필요하므로 PR 을 만든 뒤에야 링크를 만들 수 있다** —
 * `/rocky:finish` 는 PR 생성 → 번호 확보 → 본문 갱신 순으로 돈다.
 */
export function buildDiffLink(input: {
  slug: RepoSlug;
  prNumber: number;
  path: string;
  line?: { start: number; end: number };
}): string {
  const { slug, prNumber, path, line } = input;
  const fileHash = new Bun.CryptoHasher('sha256').update(path).digest('hex');
  const base = `https://github.com/${slug.owner}/${slug.repo}/pull/${prNumber}/files#diff-${fileHash}`;
  if (!line) {
    return base;
  }
  return line.end > line.start ? `${base}R${line.start}-R${line.end}` : `${base}R${line.start}`;
}

/** `https://github.com/<owner>/<repo>/blob/<sha>/<path>#L<start>-L<end>` 를 만든다. */
export function buildPermalink(input: {
  slug: RepoSlug;
  sha: string;
  path: string;
  line?: { start: number; end: number };
}): string {
  const { slug, sha, path, line } = input;
  const encoded = path.split('/').map(encodeURIComponent).join('/');
  const base = `https://github.com/${slug.owner}/${slug.repo}/blob/${sha}/${encoded}`;
  if (!line) {
    return base;
  }
  return line.end > line.start ? `${base}#L${line.start}-L${line.end}` : `${base}#L${line.start}`;
}

function git(args: string[]): string {
  const result = Bun.spawnSync(['git', ...args]);
  if (result.exitCode !== 0) {
    throw new Error(
      `git ${args.join(' ')} 실패 (exit ${result.exitCode}) — ${result.stderr.toString().trim()}`,
    );
  }
  return result.stdout.toString().trim();
}

async function main(argv: string[]): Promise<number> {
  const urlOnly = argv.includes('--url');
  const prIndex = argv.indexOf('--pr');
  const prNumber = prIndex === -1 ? undefined : Number(argv[prIndex + 1]);
  if (prIndex !== -1 && (!prNumber || !Number.isInteger(prNumber))) {
    console.error(`--pr 에 PR 번호가 필요하다 — 받은 값=${argv[prIndex + 1] ?? '(없음)'}`);
    return 2;
  }
  const skip = prIndex === -1 ? new Set<number>() : new Set([prIndex, prIndex + 1]);
  const pointers = argv.filter((arg, i) => arg !== '--url' && !skip.has(i));
  if (pointers.length === 0) {
    console.error(
      '사용법: bun scripts/permalink.ts [--pr <번호>] [--url] <경로[:심볼|:줄|:시작-끝]> ...\n' +
        '예: bun scripts/permalink.ts --pr 127 commands/finish.md:12-18\n' +
        '--pr 이면 그 PR 의 Files changed 위치로, 없으면 blob permalink 로 건다.\n' +
        '기본 출력은 `[경로:줄](URL)` 마크다운 링크. --url 이면 날 URL 만 출력한다.',
    );
    return 2;
  }

  const sha = git(['rev-parse', 'HEAD']);
  const slug = parseRepoSlug(git(['remote', 'get-url', 'origin']));
  const root = git(['rev-parse', '--show-toplevel']);

  // 푸시되지 않은 커밋의 permalink 는 GitHub 에서 404 다. 링크를 만들되 경고는 남긴다.
  const onRemote = Bun.spawnSync(['git', 'branch', '-r', '--contains', sha])
    .stdout.toString()
    .trim();
  if (!onRemote) {
    console.error(`경고: ${sha.slice(0, 7)} 는 아직 원격에 없다 — 푸시해야 링크가 열린다.`);
  }

  let failed = false;
  for (const raw of pointers) {
    try {
      const pointer = parsePointer(raw);
      let line = pointer.line;
      if (pointer.symbol) {
        const file = Bun.file(`${root}/${pointer.path}`);
        if (!(await file.exists())) {
          throw new Error(`파일이 없다 — ${pointer.path} (저장소 루트=${root})`);
        }
        const found = resolveSymbolLine(await file.text(), pointer.symbol, pointer.path);
        line = { start: found, end: found };
      }
      const url = prNumber
        ? buildDiffLink({ slug, prNumber, path: pointer.path, line })
        : buildPermalink({ slug, sha, path: pointer.path, line });
      // 기본은 마크다운 링크 — PR 본문에 그대로 붙여 쓰는 형태다. 날 URL 이 필요하면 --url.
      console.log(urlOnly ? url : `[${formatPointerLabel(pointer.path, line)}](${url})`);
    } catch (error) {
      failed = true;
      console.error(`${raw} → ${error instanceof Error ? error.message : String(error)}`);
    }
  }
  return failed ? 1 : 0;
}

if (import.meta.main) {
  process.exit(await main(process.argv.slice(2)));
}
