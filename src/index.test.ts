/**
 * Smoke tests for the Claude Code MCP server entrypoint. Connects an
 * in-memory `Client` to a real `buildServer()` instance and asserts:
 *
 *  - the expected 7 openapi tools are registered
 *  - no removed-domain tools (notion / journal / mysql / spec-pact / pr-watch) leak
 *  - openapi tool input schemas advertise the right required fields
 *
 * No network — only the surface that does not require external services is
 * exercised here. Handler logic itself is covered by openapi-core's own test
 * suite.
 */

import { afterAll, beforeAll, describe, expect, test } from 'bun:test';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { InMemoryTransport } from '@modelcontextprotocol/sdk/inMemory.js';
import { buildServer } from './index';

const EXPECTED_TOOLS = [
  'openapi_get',
  'openapi_refresh',
  'openapi_status',
  'openapi_search',
  'openapi_envs',
  'openapi_endpoint',
  'openapi_tags',
] as const;

/** v0.3 부터 surface 에서 빠진 tool — 누수 회귀 가드. archive/pre-openapi-only-slim 참조. */
const REMOVED_TOOLS = [
  'notion_get',
  'notion_refresh',
  'notion_status',
  'notion_extract',
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

const ENV_KEYS_TO_RESTORE = ['AGENT_TOOLKIT_OPENAPI_CACHE_DIR'] as const;

let client: Client;
let tmpHome: string;
const savedEnv: Record<string, string | undefined> = {};

beforeAll(async () => {
  tmpHome = mkdtempSync(join(tmpdir(), 'agent-toolkit-server-test-'));
  for (const key of ENV_KEYS_TO_RESTORE) {
    savedEnv[key] = process.env[key];
  }
  process.env.AGENT_TOOLKIT_OPENAPI_CACHE_DIR = join(tmpHome, 'openapi-cache');

  const server = await buildServer();
  const [clientTransport, serverTransport] = InMemoryTransport.createLinkedPair();

  client = new Client({ name: 'agent-toolkit-test', version: '0' });
  await Promise.all([server.connect(serverTransport), client.connect(clientTransport)]);
});

afterAll(async () => {
  await client?.close().catch(() => undefined);
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

describe('agent-toolkit Claude Code MCP server', () => {
  test('exposes exactly the 7 openapi tools', async () => {
    const result = await client.listTools();
    const names = result.tools.map((t) => t.name).sort();
    expect(names).toEqual([...EXPECTED_TOOLS].sort());
  });

  test('does not leak removed-domain tools', async () => {
    const result = await client.listTools();
    const names = new Set(result.tools.map((t) => t.name));
    for (const removed of REMOVED_TOOLS) {
      expect(names.has(removed)).toBe(false);
    }
  });

  // NOTE: 이 required 필드는 플러그인(handle 기반) contract 이다. standalone CLI
  // (`src/standalone.ts`) 는 openapi_endpoint 가 spec+environment 를 요구하는 등
  // 형태가 다르다 — 그쪽 계약은 이 테스트 대상이 아니다.
  test('advertises the expected required fields per openapi tool', async () => {
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
  });
});
