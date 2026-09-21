/**
 * Smoke tests for the Claude Code MCP server entrypoint. Connects an
 * in-memory `Client` to a real `buildServer()` instance and asserts:
 *
 *  - exactly the 4 worklog_* tools are registered
 *  - no removed-domain tool (openapi / seo / notion / mysql / spec-pact / pr-watch) leaks back
 *  - worklog tool input schemas advertise the right required fields
 *
 * No network / no subprocess — the worklog lands in a tmpdir via ROCKY_WORKLOG_DIR.
 */

import { afterAll, beforeAll, describe, expect, test } from 'bun:test';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { InMemoryTransport } from '@modelcontextprotocol/sdk/inMemory.js';
import { type BuildServerOptions, buildServer } from './index';

/** worklog_* 는 기록 레이어 — 게이트 없이 항상 등록. 현재 rocky 의 MCP 표면 전부다. */
const WORKLOG_TOOLS = [
  'worklog_append',
  'worklog_read',
  'worklog_search',
  'worklog_status',
] as const;

/** 제거된 도메인의 tool — surface 에 다시 새어 들어오면 안 된다 (누수 회귀 가드). */
const REMOVED_TOOLS = [
  // v0.23 — 39개 레포 5,216 턴에서 호출 0회라 걷어냄. git 히스토리에만 남는다.
  'openapi_get',
  'openapi_refresh',
  'openapi_status',
  'openapi_search',
  'openapi_envs',
  'openapi_endpoint',
  'openapi_tags',
  'seo_validate',
  'notion_get',
  'notion_refresh',
  'notion_status',
  'notion_extract',
  // v0.3 — archive/pre-openapi-only-slim
  'mysql_envs',
  'mysql_status',
  'mysql_tables',
  'mysql_schema',
  'mysql_query',
  'spec_pact_fragment',
  'pr_watch_start',
  'pr_watch_stop',
  'pr_watch_status',
  'pr_event_record',
  'pr_event_pending',
  'pr_event_resolve',
] as const;

let tmpHome: string;
let savedWorklogDir: string | undefined;

async function connect(options: BuildServerOptions = {}): Promise<Client> {
  const server = await buildServer(options);
  const [clientTransport, serverTransport] = InMemoryTransport.createLinkedPair();
  const client = new Client({ name: 'rocky-test', version: '0' });
  await Promise.all([server.connect(serverTransport), client.connect(clientTransport)]);
  return client;
}

async function toolNames(client: Client): Promise<Set<string>> {
  const result = await client.listTools();
  return new Set(result.tools.map((t) => t.name));
}

beforeAll(() => {
  tmpHome = mkdtempSync(join(tmpdir(), 'rocky-server-test-'));
  savedWorklogDir = process.env.ROCKY_WORKLOG_DIR;
  process.env.ROCKY_WORKLOG_DIR = join(tmpHome, 'worklog');
});

afterAll(() => {
  if (savedWorklogDir === undefined) {
    delete process.env.ROCKY_WORKLOG_DIR;
  } else {
    process.env.ROCKY_WORKLOG_DIR = savedWorklogDir;
  }
  if (tmpHome) {
    rmSync(tmpHome, { recursive: true, force: true });
  }
});

describe('rocky Claude Code MCP server', () => {
  test('exposes exactly the worklog tools', async () => {
    const client = await connect();
    try {
      const names = [...(await toolNames(client))].sort();
      expect(names).toEqual([...WORKLOG_TOOLS].sort());
    } finally {
      await client.close().catch(() => undefined);
    }
  });

  test('worklog_status reports exists=false without wikiDir fields', async () => {
    const client = await connect();
    try {
      const result = await client.callTool({ name: 'worklog_status', arguments: {} });
      const content = (result.content as Array<{ type: string; text: string }>)[0];
      const parsed = JSON.parse(content!.text);
      expect(parsed.exists).toBe(false);
      expect(parsed.totalEntries).toBe(0);
      expect(parsed.wikiDir).toBeUndefined();
      expect(parsed.wikiDirSource).toBeUndefined();
      expect(typeof parsed.dirSource).toBe('string');
    } finally {
      await client.close().catch(() => undefined);
    }
  });

  test('worklog_append then worklog_read round-trips through the tool surface', async () => {
    const client = await connect();
    try {
      await client.callTool({
        name: 'worklog_append',
        arguments: { content: 'decided on 2-layer design', kind: 'decision', tags: ['worklog'] },
      });
      const read = await client.callTool({
        name: 'worklog_read',
        arguments: { kind: 'decision' },
      });
      const content = (read.content as Array<{ type: string; text: string }>)[0];
      const parsed = JSON.parse(content!.text) as Array<{ content: string; kind: string }>;
      expect(parsed[0]?.content).toBe('decided on 2-layer design');
      expect(parsed[0]?.kind).toBe('decision');
    } finally {
      await client.close().catch(() => undefined);
    }
  });

  test('does not leak removed-domain tools', async () => {
    const client = await connect();
    try {
      const names = await toolNames(client);
      for (const removed of REMOVED_TOOLS) {
        expect(names.has(removed)).toBe(false);
      }
    } finally {
      await client.close().catch(() => undefined);
    }
  });

  test('advertises the expected required fields per worklog tool', async () => {
    const client = await connect();
    try {
      const { tools } = await client.listTools();
      const byName = new Map(tools.map((t) => [t.name, t]));
      const requiredFields = (name: string): string[] => {
        const tool = byName.get(name);
        if (!tool) {
          throw new Error(`tool not found: ${name}`);
        }
        const schema = tool.inputSchema as { required?: string[] };
        return [...(schema.required ?? [])].sort();
      };
      expect(requiredFields('worklog_append')).toEqual(['content']);
      expect(requiredFields('worklog_search')).toEqual(['query']);
      expect(requiredFields('worklog_read')).toEqual([]);
      expect(requiredFields('worklog_status')).toEqual([]);
    } finally {
      await client.close().catch(() => undefined);
    }
  });
});
