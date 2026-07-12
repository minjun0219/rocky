import { type RawNotionPage, resolveCacheKey } from './notion-cache';

/**
 * Notion 접근을 외부 `ntn` CLI (공식 Notion CLI) 위임으로 처리한다.
 *
 * rocky 는 Notion API 토큰 / OAuth 를 직접 다루지 않는다 — `ntn login` 으로 인증된
 * CLI 가 있으면 그걸 통해 페이지를 가져오고, 없으면 notion_* 도구 자체를 등록하지 않는다
 * (`gh` CLI 위임과 동일한 정책). 실제 spawn 은 `NotionCliExecutor` 뒤에 숨겨 테스트에서
 * fake executor 로 대체할 수 있게 한다.
 */

/** CLI 한 번 실행 결과. */
export interface NotionCliRunResult {
  stdout: string;
  stderr: string;
  exitCode: number;
}

/** spawn 을 추상화한 실행기. 테스트는 이 인터페이스를 fake 로 구현한다. */
export interface NotionCliExecutor {
  run(args: string[], options?: { timeoutMs?: number }): Promise<NotionCliRunResult>;
}

/** CLI 바이너리를 찾을 수 없을 때 (PATH 부재). detect 단계에서 false 로 흡수된다. */
export class NotionCliNotInstalledError extends Error {
  constructor(readonly bin: string) {
    super(
      `Notion CLI "${bin}" not found on PATH. Install it and run \`${bin} login\`, or set ROCKY_NOTION_CLI.`,
    );
    this.name = 'NotionCliNotInstalledError';
  }
}

/** CLI 가 non-zero 로 끝났을 때. stderr 를 메시지에 포함한다. */
export class NotionCliCommandError extends Error {
  constructor(
    readonly args: string[],
    readonly exitCode: number,
    readonly stderr: string,
  ) {
    super(
      `Notion CLI failed (exit ${exitCode}): ${notionCliBin()} ${args.join(' ')}\n${stderr.trim().slice(0, 500)}`,
    );
    this.name = 'NotionCliCommandError';
  }
}

export const NOTION_CLI_DEFAULT_BIN = 'ntn';
export const NOTION_CLI_DEFAULT_TIMEOUT_MS = 15_000;

/** 사용할 CLI 바이너리. `ROCKY_NOTION_CLI` 로 오버라이드 (기본 `ntn`). */
export function notionCliBin(): string {
  const override = process.env.ROCKY_NOTION_CLI?.trim();
  return override && override.length > 0 ? override : NOTION_CLI_DEFAULT_BIN;
}

function notionCliTimeout(): number {
  const raw = process.env.ROCKY_NOTION_CLI_TIMEOUT_MS;
  const parsed = raw ? Number.parseInt(raw, 10) : Number.NaN;
  return Number.isFinite(parsed) && parsed > 0 ? parsed : NOTION_CLI_DEFAULT_TIMEOUT_MS;
}

/**
 * Bun.spawn 백엔드. 바이너리가 없으면 (ENOENT) `NotionCliNotInstalledError` 로 매핑한다 —
 * 그래야 detect 단계가 "미설치" 를 깨끗하게 판정한다.
 */
export function createBunNotionCli(bin: string = notionCliBin()): NotionCliExecutor {
  return {
    async run(args, options): Promise<NotionCliRunResult> {
      const timeoutMs = options?.timeoutMs ?? notionCliTimeout();
      const controller = new AbortController();
      const timer = setTimeout(() => controller.abort(), timeoutMs);
      let proc: ReturnType<typeof Bun.spawn>;
      try {
        proc = Bun.spawn([bin, ...args], {
          stdout: 'pipe',
          stderr: 'pipe',
          stdin: 'ignore',
          signal: controller.signal,
        });
      } catch (err) {
        clearTimeout(timer);
        // Bun 은 바이너리 부재 시 spawn 단계에서 던진다.
        if (isEnoent(err)) {
          throw new NotionCliNotInstalledError(bin);
        }
        throw err;
      }
      try {
        const [stdout, stderr, exitCode] = await Promise.all([
          new Response(proc.stdout as ReadableStream<Uint8Array>).text(),
          new Response(proc.stderr as ReadableStream<Uint8Array>).text(),
          proc.exited,
        ]);
        return { stdout, stderr, exitCode };
      } catch (err) {
        if (controller.signal.aborted) {
          throw new NotionCliCommandError(args, 124, `Notion CLI timed out after ${timeoutMs}ms`);
        }
        if (isEnoent(err)) {
          throw new NotionCliNotInstalledError(bin);
        }
        throw err;
      } finally {
        clearTimeout(timer);
      }
    },
  };
}

function isEnoent(err: unknown): boolean {
  const code = (err as { code?: string } | null)?.code;
  return (
    code === 'ENOENT' ||
    (err instanceof Error && /ENOENT|not found|no such file/i.test(err.message))
  );
}

/**
 * CLI 가 설치 + 로그인되어 있는지 가볍게 확인한다. `--version` 이 0 으로 끝나면 true.
 * 미설치 / 오류는 전부 false 로 흡수 — detect 는 절대 던지지 않는다 (도구 등록 게이트용).
 */
export async function detectNotionCli(exec: NotionCliExecutor): Promise<boolean> {
  try {
    const result = await exec.run(['--version'], { timeoutMs: 5_000 });
    return result.exitCode === 0;
  } catch {
    return false;
  }
}

/**
 * `ntn pages get <input> --json` 을 실행해 RawNotionPage 로 정규화한다.
 *
 * 파서는 관용적이다 — `ntn` 의 JSON 이 markdown / content / body 중 어느 키에 본문을 담든,
 * title / name 중 어디에 제목을 담든 흡수한다. id 는 payload 에 있으면 쓰고 없으면 입력에서
 * 유도한다. JSON 파싱 자체가 실패하면 stdout 전체를 markdown 으로 취급한다 (plain
 * `ntn pages get` 출력 fallback).
 */
export async function notionCliFetch(
  exec: NotionCliExecutor,
  input: string,
): Promise<RawNotionPage> {
  const { pageId } = resolveCacheKey(input);
  const args = ['pages', 'get', input, '--json'];
  const result = await exec.run(args);
  if (result.exitCode !== 0) {
    throw new NotionCliCommandError(args, result.exitCode, result.stderr);
  }
  return parseNtnPayload(result.stdout, input, pageId);
}

/** RawNotionPage 로 정규화. export 는 단위 테스트용. */
export function parseNtnPayload(stdout: string, input: string, pageId: string): RawNotionPage {
  const text = stdout.trim();
  if (text.length === 0) {
    throw new Error(`Notion CLI returned empty output for "${input}"`);
  }

  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch {
    // JSON 이 아니면 plain markdown 출력으로 간주.
    return { id: deriveId(input, pageId), title: titleFromMarkdown(text), markdown: text };
  }

  if (parsed === null || typeof parsed !== 'object') {
    return { id: deriveId(input, pageId), title: titleFromMarkdown(text), markdown: text };
  }

  const obj = parsed as Record<string, unknown>;
  const markdown = firstString(obj, ['markdown', 'content', 'body', 'text']) ?? '';
  const remoteId = firstString(obj, ['id', 'page_id', 'pageId']);
  const title =
    firstString(obj, ['title', 'name']) ??
    notionTitleFromProperties(obj) ??
    titleFromMarkdown(markdown);

  return {
    id: remoteId ? deriveId(remoteId, pageId) : deriveId(input, pageId),
    title: title || '(untitled)',
    markdown,
  };
}

/** 후보 키 중 첫 non-empty string 을 반환. */
function firstString(obj: Record<string, unknown>, keys: string[]): string | undefined {
  for (const key of keys) {
    const value = obj[key];
    if (typeof value === 'string' && value.trim().length > 0) {
      return value;
    }
  }
  return undefined;
}

/** Notion API page 객체의 `properties.*.title[].plain_text` 에서 제목을 시도. */
function notionTitleFromProperties(obj: Record<string, unknown>): string | undefined {
  const props = obj.properties;
  if (props === null || typeof props !== 'object') {
    return undefined;
  }
  for (const value of Object.values(props as Record<string, unknown>)) {
    if (value === null || typeof value !== 'object') {
      continue;
    }
    const titleArr = (value as Record<string, unknown>).title;
    if (Array.isArray(titleArr)) {
      const joined = titleArr
        .map((t) =>
          t && typeof t === 'object' ? String((t as Record<string, unknown>).plain_text ?? '') : '',
        )
        .join('')
        .trim();
      if (joined.length > 0) {
        return joined;
      }
    }
  }
  return undefined;
}

/** remote id 가 page id 로 해석되면 정규화, 아니면 입력 기반 pageId 로 fallback. */
function deriveId(candidate: string, fallbackPageId: string): string {
  try {
    return resolveCacheKey(candidate).pageId;
  } catch {
    return fallbackPageId;
  }
}

/** markdown 본문의 첫 heading 을 제목으로. frontmatter 는 건너뛴다. */
function titleFromMarkdown(markdown: string): string {
  const lines = markdown.split(/\r?\n/);
  let inFrontmatter = false;
  for (let i = 0; i < lines.length; i += 1) {
    const line = (lines[i] ?? '').trim();
    if (i === 0 && line === '---') {
      inFrontmatter = true;
      continue;
    }
    if (inFrontmatter) {
      if (line === '---') {
        inFrontmatter = false;
      }
      continue;
    }
    const heading = line.match(/^#{1,6}\s+(.+?)\s*#*$/);
    if (heading) {
      return heading[1]!.trim();
    }
  }
  return '(untitled)';
}
