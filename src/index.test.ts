/**
 * Smoke tests for the Claude Code MCP server entrypoint. Connects an
 * in-memory `Client` to a real `buildServer()` instance and asserts:
 *
 *  - the expected 7 openapi tools + seo_validate are registered
 *  - notion_* tools appear only when a Notion CLI (`ntn`) is detected — the
 *    detection is stubbed via an injected executor so the suite is deterministic
 *    regardless of whether `ntn` is on the CI PATH
 *  - no other removed-domain tools (journal / mysql / spec-pact / pr-watch) leak
 *  - openapi tool input schemas advertise the right required fields
 *
 * No network / no real subprocess — the notion CLI is always a fake executor.
 */

import { afterAll, beforeAll, describe, expect, test } from 'bun:test';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { InMemoryTransport } from '@modelcontextprotocol/sdk/inMemory.js';
import type { NotionCliExecutor } from './core';
import { type BuildServerOptions, buildServer } from './index';

const OPENAPI_TOOLS = [
  'openapi_get',
  'openapi_refresh',
  'openapi_status',
  'openapi_search',
  'openapi_envs',
  'openapi_endpoint',
  'openapi_tags',
  'seo_validate',
] as const;

const NOTION_TOOLS = ['notion_get', 'notion_refresh', 'notion_status', 'notion_extract'] as const;

/** v0.3 부터 surface 에서 빠진 tool — 누수 회귀 가드. archive/pre-openapi-only-slim 참조. */
const REMOVED_TOOLS = [
  'journal_append',
  'journal_read',
  'journal_search',
  'journal_status',
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

const PAGE_DASHED = '1234abcd-1234-abcd-1234-abcd1234abcd';

/** `ntn` 미설치 시뮬레이션 — detect 가 false 로 떨어진다. */
const absentNotionCli: NotionCliExecutor = {
  async run() {
    throw new Error('ENOENT: ntn not found');
  },
};

/** `ntn` 설치 + 로그인 시뮬레이션 — --version 성공, pages get 은 고정 JSON 반환. */
const presentNotionCli: NotionCliExecutor = {
  async run(args) {
    if (args[0] === '--version') {
      return { stdout: 'ntn 0.19.0', stderr: '', exitCode: 0 };
    }
    if (args[0] === 'pages' && args[1] === 'get') {
      return {
        stdout: JSON.stringify({
          id: PAGE_DASHED,
          title: 'Fixture',
          markdown: '# Fixture\n\nbody',
        }),
        stderr: '',
        exitCode: 0,
      };
    }
    return { stdout: '', stderr: 'unexpected', exitCode: 1 };
  },
};

const ENV_KEYS_TO_RESTORE = ['ROCKY_OPENAPI_CACHE_DIR', 'ROCKY_NOTION_CACHE_DIR'] as const;

let tmpHome: string;
const savedEnv: Record<string, string | undefined> = {};

async function connect(options: BuildServerOptions): Promise<Client> {
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
  for (const key of ENV_KEYS_TO_RESTORE) {
    savedEnv[key] = process.env[key];
  }
  process.env.ROCKY_OPENAPI_CACHE_DIR = join(tmpHome, 'openapi-cache');
  process.env.ROCKY_NOTION_CACHE_DIR = join(tmpHome, 'notion-cache');
});

afterAll(() => {
  for (const key of ENV_KEYS_TO_RESTORE) {
    const prior = savedEnv[key];
    if (prior === undefined) {
      delete process.env[key];
    } else {
      process.env[key] = prior;
    }
  }
  if (tmpHome) {
    rmSync(tmpHome, { recursive: true, force: true });
  }
});

describe('rocky Claude Code MCP server', () => {
  test('without a Notion CLI, exposes exactly the openapi tools + seo_validate', async () => {
    const client = await connect({ notionCli: absentNotionCli });
    try {
      const names = [...(await toolNames(client))].sort();
      expect(names).toEqual([...OPENAPI_TOOLS].sort());
    } finally {
      await client.close().catch(() => undefined);
    }
  });

  test('does not leak removed-domain tools', async () => {
    const client = await connect({ notionCli: absentNotionCli });
    try {
      const names = await toolNames(client);
      for (const removed of REMOVED_TOOLS) {
        expect(names.has(removed)).toBe(false);
      }
    } finally {
      await client.close().catch(() => undefined);
    }
  });

  test('advertises the expected required fields per openapi tool', async () => {
    const client = await connect({ notionCli: absentNotionCli });
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
      expect(requiredFields('openapi_get')).toEqual(['input']);
      expect(requiredFields('openapi_search')).toEqual(['query']);
      expect(requiredFields('openapi_endpoint')).toEqual(['input']);
      expect(requiredFields('openapi_tags')).toEqual(['input']);
      expect(requiredFields('openapi_envs')).toEqual([]);
    } finally {
      await client.close().catch(() => undefined);
    }
  });
});

describe('notion tools (CLI-gated)', () => {
  test('registers notion_* when a Notion CLI is detected', async () => {
    const client = await connect({ notionCli: presentNotionCli });
    try {
      const names = await toolNames(client);
      for (const tool of NOTION_TOOLS) {
        expect(names.has(tool)).toBe(true);
      }
      // openapi surface 는 그대로 공존한다.
      expect(names.has('openapi_get')).toBe(true);
      expect(names.has('seo_validate')).toBe(true);
    } finally {
      await client.close().catch(() => undefined);
    }
  });

  test('omits notion_* when no Notion CLI is present', async () => {
    const client = await connect({ notionCli: absentNotionCli });
    try {
      const names = await toolNames(client);
      for (const tool of NOTION_TOOLS) {
        expect(names.has(tool)).toBe(false);
      }
    } finally {
      await client.close().catch(() => undefined);
    }
  });

  test('notion_status returns exists=false for an uncached page (no CLI call)', async () => {
    const client = await connect({ notionCli: presentNotionCli });
    try {
      const result = await client.callTool({
        name: 'notion_status',
        arguments: { input: 'abcdef00-0000-0000-0000-000000000000' },
      });
      const content = (result.content as Array<{ type: string; text: string }>)[0];
      const parsed = JSON.parse(content!.text);
      expect(parsed.exists).toBe(false);
    } finally {
      await client.close().catch(() => undefined);
    }
  });
});
