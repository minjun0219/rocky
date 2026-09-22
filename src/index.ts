/**
 * Claude Code MCP server entrypoint for rocky.
 *
 * stdio JSON-RPC server that exposes the 4 `worklog_*` tools — the record (記錄) layer. The
 * organize (整理) layer is the `/rocky:recall` slash command on the host LLM.
 *
 * openapi_* / seo_validate / notion_* were removed in v0.23 after a usage count across 39 repos
 * showed zero calls; they live in git history only. mysql / spec-pact / pr-watch are older still,
 * on `archive/pre-openapi-only-slim`.
 */

import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import { z } from 'zod';
import pkg from '../package.json' with { type: 'json' };
import {
  type Worklog,
  createWorklogFromEnv,
  handleWorklogAppend,
  handleWorklogRead,
  handleWorklogSearch,
  handleWorklogStatus,
  loadConfig,
} from './core';

/**
 * MCP `tools/call` results must be a `CallToolResult`. We always serialize the
 * handler return value as a single JSON text content block.
 */
function jsonResult(value: unknown) {
  return {
    content: [
      {
        type: 'text' as const,
        text: JSON.stringify(value, null, 2),
      },
    ],
  };
}

/** `buildServer` 주입 옵션. 테스트가 tmpdir 워크로그로 대체할 때 쓴다. */
export interface BuildServerOptions {
  /**
   * 워크로그 인스턴스 주입. 미지정이면 `createWorklogFromEnv(config.worklog)` 로 만든다
   * (env `ROCKY_WORKLOG_DIR` > `rocky.json` 의 worklog 키 > 프로젝트별 기본 경로).
   */
  worklog?: Worklog;
}

/**
 * Build the MCP server with the worklog tools wired up. Exported for tests so they can register
 * tools against an in-process server without spawning a child.
 */
export async function buildServer(options: BuildServerOptions = {}) {
  const { config: toolkitConfig, errors: configErrors } = await loadConfig();
  for (const e of configErrors) {
    console.error(
      `rocky: skipped config file ${e.source} — ${e.message}. Other config sources still apply.`,
    );
  }

  const server = new McpServer({
    name: 'rocky',
    version: pkg.version,
  });

  // worklog_* 는 기록(記錄) 레이어 — append-only 로컬 JSONL. 외부 의존이 없어(순수 파일
  // 시스템) 무조건 등록한다. `Stop` hook (src/hooks/log-turn.ts) 이 매 턴 종료 시 kind:"turn" 을
  // 자동으로 남기고, 정리(整理: 앵커 히스토리 다이제스트)는 rocky 가 아니라 `/rocky:recall`
  // 슬래시커맨드(호스트 LLM)의 몫이다.
  const worklog = options.worklog ?? createWorklogFromEnv(toolkitConfig.worklog);
  server.registerTool(
    'worklog_append',
    {
      description:
        '워크로그에 한 줄을 append-only 로 기록한다. 다음 turn 에 인용할 결정 / blocker / 사용자 답변 / 메모를 남길 때 사용. remote 호출 없음. 저장 위치는 `worklog.dir`(rocky.json) 또는 `ROCKY_WORKLOG_DIR`(env 우선)로 변경 가능(worklog_status 로 확인). (content: 필수 본문, kind?: decision/blocker/answer/note 등 기본 note, tags?: 문자열 배열, pageId?: 연결할 Notion page id 또는 URL)',
      inputSchema: {
        content: z.string(),
        kind: z.string().optional(),
        tags: z.array(z.string()).optional(),
        pageId: z.string().optional(),
      },
    },
    async ({ content, kind, tags, pageId }) =>
      jsonResult(await handleWorklogAppend(worklog, { content, kind, tags, pageId })),
  );
  server.registerTool(
    'worklog_read',
    {
      description:
        '저널을 가장 최근 항목부터 필터 / limit 적용해 반환한다. 손상된 라인은 자동 skip. remote 호출 없음. (limit?: 기본 20, kind?: 정확 일치, tag?: 태그 포함, pageId?: 정규화 후 일치, since?: 해당 시각 이후 ISO8601)',
      inputSchema: {
        limit: z.number().int().positive().optional(),
        kind: z.string().optional(),
        tag: z.string().optional(),
        pageId: z.string().optional(),
        since: z.string().optional(),
      },
    },
    async ({ limit, kind, tag, pageId, since }) =>
      jsonResult(await handleWorklogRead(worklog, { limit, kind, tag, pageId, since })),
  );
  server.registerTool(
    'worklog_search',
    {
      description:
        '저널을 substring (case-insensitive) 으로 검색한다. content / kind / tags / pageId 를 매칭. remote 호출 없음. (query: 검색어, limit?: 기본 20, kind?: 풀 스코프 필터)',
      inputSchema: {
        query: z.string(),
        limit: z.number().int().positive().optional(),
        kind: z.string().optional(),
      },
    },
    async ({ query, limit, kind }) =>
      jsonResult(await handleWorklogSearch(worklog, query, { limit, kind })),
  );
  server.registerTool(
    'worklog_status',
    {
      description:
        '워크로그 메타(파일 경로, 존재 여부, 유효 항목 수 — 손상 라인 skip, 바이트 크기, 마지막 항목 시각) + 마지막 digest watermark(lastDigestAt) + 경로 출처(dirSource)를 조회한다. `/recall` 이 정리 시작 시 이걸로 증분 기준점을 확인한다. remote 호출 없음. 저장 위치는 `worklog.dir`(rocky.json) 또는 `ROCKY_WORKLOG_DIR`(env 우선)로 변경 가능하다.',
      inputSchema: {},
    },
    async () => jsonResult(await handleWorklogStatus(worklog)),
  );

  return server;
}

/**
 * Entrypoint when run as the MCP server binary. Tests import `buildServer`
 * directly and never hit this branch.
 */
async function main() {
  const server = await buildServer();
  const transport = new StdioServerTransport();
  await server.connect(transport);
}

if (import.meta.main) {
  main().catch((err) => {
    console.error(`rocky MCP server failed: ${(err as Error).stack ?? err}`);
    process.exit(1);
  });
}
