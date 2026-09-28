import { describe, expect, test } from 'bun:test';
import { Awareness } from 'y-protocols/awareness';
import * as Y from 'yjs';
import {
  bridgeAwareness,
  colorFor,
  EDITOR_PREF_KEY,
  readEditorPref,
  writeEditorPref,
} from './codemirror-editor';

test('colorFor is stable per name and always a palette color', () => {
  expect(colorFor('codex')).toBe(colorFor('codex'));
  expect(colorFor('logan')).toMatch(/^#[0-9a-f]{6}$/);
});

describe('bridgeAwareness', () => {
  function fakeSync() {
    const pings: unknown[] = [];
    return {
      pings,
      ping: async (state?: unknown) => {
        pings.push(state);
      },
      onPresenceState: null as ((client: string, state: unknown) => void) | null,
    };
  }

  test('local awareness changes ride the presence ping', async () => {
    const doc = new Y.Doc();
    const awareness = new Awareness(doc);
    const sync = fakeSync();
    const bridge = bridgeAwareness(awareness, sync);
    awareness.setLocalStateField('user', { name: 'logan', color: '#000' });
    await Promise.resolve();
    expect(sync.pings.length).toBe(1);
    expect(sync.pings[0]).toMatchObject({ awareness: expect.any(String) });
    expect(bridge.pingState()).toMatchObject({ awareness: expect.any(String) });
    bridge.dispose();
    awareness.setLocalStateField('user', { name: 'x', color: '#111' });
    await Promise.resolve();
    expect(sync.pings.length).toBe(1);
  });

  test("another client's presence state lands in awareness, and remote applies do not echo", async () => {
    const doc = new Y.Doc();
    const mine = new Awareness(doc);
    const sync = fakeSync();
    bridgeAwareness(mine, sync);
    // 상대 — 같은 문서 구조의 다른 awareness.
    const theirs = new Awareness(new Y.Doc());
    theirs.setLocalStateField('user', { name: 'codex', color: '#c00' });
    const their = fakeSync();
    const theirBridge = bridgeAwareness(theirs, their);
    sync.onPresenceState?.('them', theirBridge.pingState());
    const states = [...mine.getStates().values()];
    expect(states.some((s) => (s as { user?: { name: string } }).user?.name === 'codex')).toBe(
      true,
    );
    await Promise.resolve();
    // 원격 적용은 내 핑을 만들지 않는다.
    expect(sync.pings.length).toBe(0);
    sync.onPresenceState?.('them', { not: 'awareness' });
    sync.onPresenceState?.('them', { awareness: '!!!' });
  });
});

test('editor preference defaults to textarea and round-trips', () => {
  const store = new Map<string, string>();
  const storage = {
    getItem: (k: string) => store.get(k) ?? null,
    setItem: (k: string, v: string) => void store.set(k, v),
  };
  expect(readEditorPref(storage)).toBe('textarea');
  writeEditorPref(storage, 'codemirror');
  expect(store.get(EDITOR_PREF_KEY)).toBe('codemirror');
  expect(readEditorPref(storage)).toBe('codemirror');
  expect(
    readEditorPref({
      getItem: () => {
        throw new Error('blocked');
      },
    }),
  ).toBe('textarea');
});
