import { randomBytes, createHash } from 'node:crypto';
import { existsSync } from 'node:fs';
import { appendFile, mkdir, open, readFile, stat } from 'node:fs/promises';
import { homedir } from 'node:os';
import { basename, join, resolve, sep } from 'node:path';
import { resolveCacheKey } from './notion-cache';

/**
 * Append-only 에이전트 저널 — 기록(記錄) 레이어.
 *
 * 한 turn 안에서 결정 / blocker / 사용자 답변 등 "다음 turn 에 인용하고 싶은 사실"
 * 을 디스크에 한 줄(JSONL) 씩 쌓는다. 캐시처럼 정규화된 키 / TTL 모델을 쓰지 않는
 * 이유는 — 캐시는 외부 source of truth 의 사본이지만, 저널은 그 자체가 source of
 * truth 이기 때문이다. 따라서 만료 / 무효화 / 덮어쓰기 없음.
 *
 * 이 클래스는 **기록·저장만** 담당한다. journal 을 읽어 지식 wiki 로 증류(整理)하는
 * 것은 rocky 가 아니라 호스트 LLM (`/rocky:curate` 슬래시커맨드) 의 몫이다 — rocky 는
 * MCP 서버라 LLM 을 내장하지 않는다. `wikiDir` 는 그 정리 대상 위치를 status 로 노출만
 * 한다 (여기서 wiki 를 쓰지는 않는다).
 *
 * 디스크 레이아웃:
 *   <baseDir>/journal.jsonl   각 줄이 하나의 JournalEntry
 *
 * baseDir 기본값은 `~/.config/rocky/journal/<project-key>` — 프로젝트별로 격리한다
 * (cwd basename + cwd 절대경로 sha1 앞 8자). `ROCKY_JOURNAL_DIR` 로 통째로 덮어쓴다.
 *
 * 동시 쓰기:
 *   `appendFile` 는 append 모드로 기록하지만, 라인 단위 비-interleaving 이 항상
 *   보장된다고 가정하지 않는다. 직전 프로세스가 mid-write 로 죽으면 마지막 줄이 `\n`
 *   없이 끝날 수 있어, append 단계에서 마지막 바이트를 peek 해 newline 이 아니면
 *   leading `\n` 을 붙이고, read 단계에서는 파싱되지 않는 줄을 graceful skip 한다.
 */

/** 프로젝트별 저널 디렉터리의 부모. `ROCKY_JOURNAL_DIR` 로 통째로 덮어쓴다. */
export const DEFAULT_JOURNAL_ROOT = join(homedir(), '.config', 'rocky', 'journal');

/** 저널 파일 이름 — 한 디렉터리에 단일 파일을 둔다 (MVP). */
export const JOURNAL_FILE = 'journal.jsonl';

/**
 * cwd 로부터 프로젝트별 디렉터리 키를 만든다.
 * `<sanitized-basename>-<sha1(absolute cwd) 앞 8자>` — 이름 충돌(같은 basename 의 서로
 * 다른 경로)을 hash 로 가른다.
 */
export function defaultProjectKey(cwd: string = process.cwd()): string {
  const root = resolve(cwd);
  const base =
    basename(root)
      .replace(/[^a-zA-Z0-9_-]+/g, '-')
      .replace(/^-+|-+$/g, '') || 'project';
  const hash = createHash('sha1').update(root).digest('hex').slice(0, 8);
  return `${base}-${hash}`;
}

/** 저널 기본 디렉터리 (프로젝트별). `ROCKY_JOURNAL_DIR` 가 있으면 그 값을 verbatim 사용. */
export function resolveDefaultJournalDir(): string {
  return join(DEFAULT_JOURNAL_ROOT, defaultProjectKey());
}

/**
 * 앞머리 `~` / `~/` 를 홈 디렉터리로 확장한다. 사용자가 `rocky.json` 의 `journal.dir` /
 * `journal.wikiDir` 나 env 에 `~/...` 를 적어도 동작하게 — `node:path` 의 `resolve` 는
 * tilde 를 확장하지 않아 그대로 두면 `<cwd>/~/...` 가 되어버린다.
 */
export function expandTilde(input: string): string {
  if (input === '~') {
    return homedir();
  }
  if (input.startsWith('~/') || input.startsWith(`~${sep}`)) {
    return join(homedir(), input.slice(2));
  }
  return input;
}

/**
 * 항목 종류. 저널은 자유 문자열을 허용하지만 흔한 값들을 권장으로 둔다.
 * `curate` 는 `/rocky:curate` 가 정리 시점을 남기는 watermark 종류 — 다음 정리는 이
 * 항목 이후만 증분 처리한다.
 */
export type JournalKind = 'decision' | 'blocker' | 'answer' | 'note' | 'curate' | (string & {});

/**
 * 저널 한 줄.
 * - `id`: timestamp + 6 hex — 같은 ms 안에 두 번 append 해도 충돌 안 나게.
 * - `timestamp`: append 시각 ISO8601 (UTC).
 * - `pageId`: 옵셔널 Notion page id 연결고리. 입력은 URL/dash-less 모두 허용하되
 *   디스크에는 정규화된 dash 형식(`8-4-4-4-12`)으로 저장.
 */
export interface JournalEntry {
  id: string;
  timestamp: string;
  kind: JournalKind;
  content: string;
  tags: string[];
  pageId?: string;
}

export interface JournalAppendInput {
  content: string;
  kind?: JournalKind;
  tags?: string[];
  pageId?: string;
}

/**
 * read 필터. 모두 옵셔널 — 다 비우면 가장 최근 `limit` (기본 20) 개를 반환.
 * `since` 는 ISO8601 또는 `Date.parse` 가 받아들이는 형식이면 된다.
 */
export interface JournalReadOptions {
  limit?: number;
  kind?: string;
  tag?: string;
  pageId?: string;
  since?: string;
}

export interface JournalSearchOptions {
  limit?: number;
  kind?: string;
}

/**
 * 저널 dir 의 해석 출처. `createJournalFromEnv` 의 우선순위와 1:1 로 대응한다:
 * env(`ROCKY_JOURNAL_DIR`) 있으면 `'env'`, 없고 config(`journal.dir`) 있으면 `'config'`,
 * 둘 다 없으면 계산된 프로젝트별 기본 경로라 `'default'`. 이 값을 status 로 노출해,
 * 소스를 안 읽어도 저장 위치가 어디서 왔는지 / 바꿀 수 있는지 발견 가능하게 한다.
 */
export type JournalDirSource = 'env' | 'config' | 'default';

/**
 * 저널 wikiDir 의 해석 출처. env(`ROCKY_JOURNAL_WIKI_DIR`) 있으면 `'env'`, 없고
 * config(`journal.wikiDir`) 있으면 `'config'`, 둘 다 없으면 `'unset'` — 미설정이라
 * `wikiDir` 필드 자체는 빠져도 이 힌트로 curate 대상이 미설정임을 발견 가능하게 한다.
 */
export type JournalWikiDirSource = 'env' | 'config' | 'unset';

export interface JournalStatus {
  path: string;
  exists: boolean;
  /** 파싱 / 정규화에 성공한 유효 항목 수. 손상된 라인은 카운트에 들어가지 않는다. */
  totalEntries: number;
  sizeBytes: number;
  lastEntryAt?: string;
  /**
   * 정리(整理) 대상 wiki 위치 (설정된 경우). rocky 는 여기에 쓰지 않는다 — `/rocky:curate`
   * 가 journal 을 읽어 이 위치로 markdown 을 컴파일한다. 미설정이면 undefined.
   */
  wikiDir?: string;
  /**
   * 저널 저장 dir(`path` 의 부모)이 어디서 왔는지. `'env'` = `ROCKY_JOURNAL_DIR`,
   * `'config'` = `rocky.json` 의 `journal.dir`, `'default'` = 프로젝트별 기본 경로.
   * 소스를 안 읽어도 저장 위치가 변경 가능함을 status 만으로 발견하게 하는 힌트.
   */
  dirSource: JournalDirSource;
  /**
   * 정리 대상 wikiDir 의 출처. `'env'` = `ROCKY_JOURNAL_WIKI_DIR`, `'config'` =
   * `rocky.json` 의 `journal.wikiDir`, `'unset'` = 미설정(그래서 `wikiDir` 필드도 없음).
   * `'unset'` 이면 curate 대상을 아직 지정하지 않았고 위 두 방법으로 설정할 수 있다는 뜻.
   */
  wikiDirSource: JournalWikiDirSource;
  /** 마지막 `kind:"curate"` watermark 의 timestamp (있으면). 증분 정리의 기준점. */
  lastCurateAt?: string;
  /**
   * 프로젝트 식별 키 (`<basename>-<hash8>`). `/rocky:curate` 가 wiki 를 프로젝트별
   * 하위 폴더 (`<wikiDir>/<projectKey>/`) 로 격리할 때 쓴다 — 한 vault 를 여러 프로젝트가
   * 공유해도 섞이지 않는다.
   */
  projectKey: string;
}

export interface AgentJournalOptions {
  baseDir?: string;
  /** 정리 대상 wiki 위치. status 로 노출만 한다 (기록 동작에는 영향 없음). */
  wikiDir?: string;
  /** 프로젝트 키 override (기본 `defaultProjectKey()`). 테스트에서 고정할 때 쓴다. */
  projectKey?: string;
  /**
   * 저널 dir 의 해석 출처 (status 노출용). `createJournalFromEnv` 가 env/config/기본값
   * 판정을 넘겨준다. 미지정이면 `baseDir` 유무로 추정한다 — 주어졌으면 `'config'`,
   * 없으면 `'default'` (직접 생성하는 테스트가 명시 없이도 합리적 값을 얻도록).
   */
  dirSource?: JournalDirSource;
  /**
   * wikiDir 의 해석 출처 (status 노출용). 미지정이면 `wikiDir` 유무로 추정한다 —
   * 주어졌으면 `'config'`, 없으면 `'unset'`.
   */
  wikiDirSource?: JournalWikiDirSource;
}

const DEFAULT_LIMIT = 20;

/**
 * 파일시스템 기반 append-only 저널.
 *
 * 외부에 노출되는 메서드는 append / read / search / status 4 가지.
 * 모든 read 경로는 손상된 라인을 건너뛰고 graceful 하게 동작한다.
 */
export class AgentJournal {
  private readonly dir: string;
  private readonly file: string;
  private readonly wikiDir?: string;
  private readonly projectKey: string;
  private readonly dirSource: JournalDirSource;
  private readonly wikiDirSource: JournalWikiDirSource;

  constructor(options: AgentJournalOptions = {}) {
    this.dir = resolve(options.baseDir ? expandTilde(options.baseDir) : resolveDefaultJournalDir());
    this.file = join(this.dir, JOURNAL_FILE);
    this.wikiDir =
      typeof options.wikiDir === 'string' && options.wikiDir.trim().length > 0
        ? resolve(expandTilde(options.wikiDir.trim()))
        : undefined;
    this.projectKey = options.projectKey ?? defaultProjectKey();
    // 출처가 명시되지 않으면 baseDir / wikiDir 유무로 추정한다 (직접 생성 경로용).
    // createJournalFromEnv 는 env/config/기본값 판정을 명시적으로 넘겨준다.
    this.dirSource = options.dirSource ?? (options.baseDir ? 'config' : 'default');
    this.wikiDirSource = options.wikiDirSource ?? (this.wikiDir ? 'config' : 'unset');
  }

  getDir(): string {
    return this.dir;
  }

  getPath(): string {
    return this.file;
  }

  getWikiDir(): string | undefined {
    return this.wikiDir;
  }

  getProjectKey(): string {
    return this.projectKey;
  }

  /**
   * 저널에 한 줄 append.
   *
   * `content` 는 trim 후 비어 있으면 throw. `pageId` 가 들어오면 `resolveCacheKey` 로
   * 정규화 후 저장 → 입력이 URL 이든 dash-less hex 든 같은 키로 묶인다.
   */
  async append(input: JournalAppendInput): Promise<JournalEntry> {
    if (!input || typeof input !== 'object') {
      throw new Error('AgentJournal.append: input must be an object');
    }
    const rawContent = typeof input.content === 'string' ? input.content : '';
    const content = rawContent.trim();
    if (!content) {
      throw new Error('AgentJournal.append: content must be a non-empty string after trim');
    }
    const kind =
      typeof input.kind === 'string' && input.kind.trim().length > 0 ? input.kind.trim() : 'note';
    const tags = Array.isArray(input.tags)
      ? input.tags
          .filter((t): t is string => typeof t === 'string')
          .map((t) => t.trim())
          .filter((t) => t.length > 0)
      : [];
    let pageId: string | undefined;
    if (typeof input.pageId === 'string' && input.pageId.trim().length > 0) {
      pageId = resolveCacheKey(input.pageId).pageId;
    }
    const entry: JournalEntry = {
      id: `${Date.now()}-${randomBytes(3).toString('hex')}`,
      timestamp: new Date().toISOString(),
      kind,
      content,
      tags,
      ...(pageId ? { pageId } : {}),
    };
    await mkdir(this.dir, { recursive: true });
    // 한 줄 = 한 entry. 직전 프로세스가 appendFile 도중 죽어 마지막 줄이 `\n` 없이 끝나
    // 있으면 새 entry 를 그대로 붙일 때 두 줄이 합쳐져 둘 다 parse 실패로 손실된다.
    // 마지막 바이트가 newline 이 아닐 때만 leading `\n` 을 붙여 라인 경계를 강제한다.
    const prefix = (await this.endsWithNewline()) ? '' : '\n';
    await appendFile(this.file, `${prefix}${JSON.stringify(entry)}\n`, 'utf8');
    return entry;
  }

  /**
   * 파일 끝 바이트가 `\n` 인지 한 바이트만 peek. 파일이 없거나 비어 있으면 true.
   * 읽기 실패는 best-effort 로 true 처리해 append 자체가 막히지 않도록 한다.
   */
  private async endsWithNewline(): Promise<boolean> {
    if (!existsSync(this.file)) {
      return true;
    }
    try {
      const st = await stat(this.file);
      if (st.size === 0) {
        return true;
      }
      const fh = await open(this.file, 'r');
      try {
        const buf = Buffer.alloc(1);
        await fh.read(buf, 0, 1, st.size - 1);
        return buf[0] === 0x0a;
      } finally {
        await fh.close();
      }
    } catch {
      return true;
    }
  }

  /**
   * 가장 최근 항목부터 `limit` 개를 반환. 필터는 AND 결합.
   * - `kind`: 정확 일치
   * - `tag`: 태그 배열 안에 포함 (정확 일치)
   * - `pageId`: `resolveCacheKey` 로 정규화 후 정확 일치
   * - `since`: 해당 시각 *이후* 항목만 (Date.parse 실패 시 필터 무시)
   */
  async read(options: JournalReadOptions = {}): Promise<JournalEntry[]> {
    const all = await this.readAll();
    let filtered: JournalEntry[] = all;
    if (typeof options.kind === 'string') {
      const k = options.kind.trim();
      if (k.length > 0) {
        filtered = filtered.filter((e) => e.kind === k);
      }
    }
    if (typeof options.tag === 'string') {
      const t = options.tag.trim();
      if (t.length > 0) {
        filtered = filtered.filter((e) => e.tags.includes(t));
      }
    }
    if (typeof options.pageId === 'string') {
      const rawPageId = options.pageId.trim();
      if (rawPageId.length > 0) {
        const pid = resolveCacheKey(rawPageId).pageId;
        filtered = filtered.filter((e) => e.pageId === pid);
      }
    }
    if (typeof options.since === 'string') {
      const since = options.since.trim();
      if (since.length > 0) {
        const sinceMs = Date.parse(since);
        if (Number.isFinite(sinceMs)) {
          filtered = filtered.filter((e) => {
            const ms = Date.parse(e.timestamp);
            return Number.isFinite(ms) && ms > sinceMs;
          });
        }
      }
    }
    const reversed = [...filtered].reverse();
    return reversed.slice(0, capOrDefault(options.limit));
  }

  /**
   * substring 검색 (case-insensitive). 매칭 대상: `content` / `kind` / `tags` / `pageId`.
   * 빈 query 는 전체 (kind 필터만 적용) 를 가장 최근부터 반환.
   */
  async search(query: string, options: JournalSearchOptions = {}): Promise<JournalEntry[]> {
    const all = await this.readAll();
    const needle = (typeof query === 'string' ? query : '').trim().toLowerCase();
    let pool: JournalEntry[] = all;
    if (typeof options.kind === 'string') {
      const k = options.kind.trim();
      if (k.length > 0) {
        pool = pool.filter((e) => e.kind === k);
      }
    }
    const matches = needle.length === 0 ? pool : pool.filter((e) => entryMatchesNeedle(e, needle));
    const reversed = [...matches].reverse();
    return reversed.slice(0, capOrDefault(options.limit));
  }

  /**
   * 저널 메타 (파일 존재, 유효 항목 수, 바이트 크기, 마지막 항목 시각) + 정리 대상
   * wikiDir + 마지막 curate watermark. `totalEntries` 는 유효 entry 만 센다.
   */
  async status(): Promise<JournalStatus> {
    if (!existsSync(this.file)) {
      return {
        path: this.file,
        exists: false,
        totalEntries: 0,
        sizeBytes: 0,
        projectKey: this.projectKey,
        dirSource: this.dirSource,
        wikiDirSource: this.wikiDirSource,
        ...(this.wikiDir ? { wikiDir: this.wikiDir } : {}),
      };
    }
    let sizeBytes = 0;
    try {
      const s = await stat(this.file);
      sizeBytes = s.size;
    } catch {
      // stat 가 깨져도 read 자체는 시도 — 부분 정보라도 surface.
    }
    const all = await this.readAll();
    const last = all[all.length - 1];
    const lastCurate = [...all].reverse().find((e) => e.kind === 'curate');
    return {
      path: this.file,
      exists: true,
      totalEntries: all.length,
      sizeBytes,
      projectKey: this.projectKey,
      dirSource: this.dirSource,
      wikiDirSource: this.wikiDirSource,
      lastEntryAt: last?.timestamp,
      ...(this.wikiDir ? { wikiDir: this.wikiDir } : {}),
      ...(lastCurate ? { lastCurateAt: lastCurate.timestamp } : {}),
    };
  }

  /**
   * 모든 valid entry 를 append 순(시간 오름차순) 으로 반환.
   *
   * 두 종류의 실패를 구분한다:
   *   - **컨텐츠 손상 / 부분 쓰기 라인** → per-line graceful skip (아래 for 루프). 저널이
   *     비어 보이지 않게 정상 라인은 살린다.
   *   - **파일 부재(ENOENT) / 경쟁적 삭제** → 빈 배열. 아직 아무것도 안 쓴 정상 상태.
   *   - **그 외 IO 오류(EACCES / EPERM / EIO …)** → rethrow. 실제 오류를 `[]` 로 삼키면
   *     read/search/status 가 조용히 비어 데이터가 사라진 것처럼 보이고 `/curate` 의
   *     증분 기준(`lastCurateAt`)이 오산된다. `append` 가 같은 상황에서 throw 하는 것과
   *     대칭을 맞춘다.
   */
  private async readAll(): Promise<JournalEntry[]> {
    if (!existsSync(this.file)) {
      return [];
    }
    let raw: string;
    try {
      raw = await readFile(this.file, 'utf8');
    } catch (err) {
      // existsSync 와 readFile 사이의 경쟁적 삭제만 [] 로 흡수하고, 나머지는 표면화.
      if ((err as NodeJS.ErrnoException).code === 'ENOENT') {
        return [];
      }
      throw err;
    }
    if (!raw) {
      return [];
    }
    const out: JournalEntry[] = [];
    for (const line of raw.split('\n')) {
      const trimmed = line.trim();
      if (!trimmed) {
        continue;
      }
      let parsed: unknown;
      try {
        parsed = JSON.parse(trimmed);
      } catch {
        continue;
      }
      const entry = normalizeEntry(parsed);
      if (entry) {
        out.push(entry);
      }
    }
    return out;
  }
}

function capOrDefault(limit: unknown): number {
  if (typeof limit !== 'number') {
    return DEFAULT_LIMIT;
  }
  if (!Number.isFinite(limit) || limit <= 0) {
    return DEFAULT_LIMIT;
  }
  return Math.floor(limit);
}

function entryMatchesNeedle(entry: JournalEntry, needle: string): boolean {
  if (entry.content.toLowerCase().includes(needle)) {
    return true;
  }
  if (entry.kind.toLowerCase().includes(needle)) {
    return true;
  }
  if (entry.pageId?.toLowerCase().includes(needle)) {
    return true;
  }
  for (const t of entry.tags) {
    if (t.toLowerCase().includes(needle)) {
      return true;
    }
  }
  return false;
}

/**
 * raw JSON 한 줄을 JournalEntry 로 정규화. 필수 필드가 비어 있거나 유효하지 않으면
 * null — read 단에서 그대로 skip 된다.
 */
function normalizeEntry(value: unknown): JournalEntry | null {
  if (!value || typeof value !== 'object') {
    return null;
  }
  const o = value as Record<string, unknown>;
  if (typeof o.id !== 'string') {
    return null;
  }
  if (typeof o.timestamp !== 'string') {
    return null;
  }
  if (typeof o.kind !== 'string') {
    return null;
  }
  if (typeof o.content !== 'string') {
    return null;
  }
  const id = o.id.trim();
  const timestamp = o.timestamp.trim();
  const kind = o.kind.trim();
  const content = o.content.trim();
  if (id.length === 0) {
    return null;
  }
  if (timestamp.length === 0 || Number.isNaN(Date.parse(timestamp))) {
    return null;
  }
  if (kind.length === 0) {
    return null;
  }
  if (content.length === 0) {
    return null;
  }
  const tags = Array.isArray(o.tags)
    ? o.tags
        .filter((t): t is string => typeof t === 'string')
        .map((t) => t.trim())
        .filter((t) => t.length > 0)
    : [];
  const pageId =
    typeof o.pageId === 'string' && o.pageId.trim().length > 0 ? o.pageId.trim() : undefined;
  return {
    id,
    timestamp,
    kind,
    content,
    tags,
    ...(pageId ? { pageId } : {}),
  };
}

/** 저널 dir / wikiDir 기본값 (env 우선). config 로 채워질 수 있는 선택 필드. */
export interface JournalEnvOptions {
  /** config.journal.dir — env `ROCKY_JOURNAL_DIR` 이 있으면 env 가 우선. */
  dir?: string;
  /** config.journal.wikiDir — env `ROCKY_JOURNAL_WIKI_DIR` 이 있으면 env 가 우선. */
  wikiDir?: string;
}

/**
 * env(`ROCKY_JOURNAL_DIR` / `ROCKY_JOURNAL_WIKI_DIR`) → config → 계산된 기본값 순으로
 * 저널 인스턴스를 만든다. env 가 명시적 per-process override 라 config 를 이긴다.
 */
export function createJournalFromEnv(config: JournalEnvOptions = {}): AgentJournal {
  // 소스별 값을 한 번만 추출해 baseDir/dirSource, wikiDir/wikiDirSource 를 파생한다.
  // firstNonEmpty 가 trim + 빈문자 처리를 하므로 `envDir ?? configDir` 는 기존
  // `firstNonEmpty(env, config)` 와 동치 — env 우선, 없으면 config, 둘 다 없으면 undefined.
  const envDir = firstNonEmpty(process.env.ROCKY_JOURNAL_DIR);
  const configDir = firstNonEmpty(config.dir);
  const baseDir = envDir ?? configDir;
  const dirSource: JournalDirSource = envDir ? 'env' : configDir ? 'config' : 'default';

  const envWiki = firstNonEmpty(process.env.ROCKY_JOURNAL_WIKI_DIR);
  const configWiki = firstNonEmpty(config.wikiDir);
  const wikiDir = envWiki ?? configWiki;
  const wikiDirSource: JournalWikiDirSource = envWiki ? 'env' : configWiki ? 'config' : 'unset';

  return new AgentJournal({ baseDir, wikiDir, dirSource, wikiDirSource });
}

function firstNonEmpty(...values: Array<string | undefined>): string | undefined {
  for (const v of values) {
    if (typeof v === 'string' && v.trim().length > 0) {
      return v.trim();
    }
  }
  return undefined;
}
