import { describe, expect, test } from 'bun:test';
import * as Y from 'yjs';
import { TEXT_KEY } from './notedoc';
import { bindTextarea } from './textarea-binding';

const LOCAL = Symbol('local');

function setup(initial: string) {
  const doc = new Y.Doc();
  const text = doc.getText(TEXT_KEY);
  text.insert(0, initial);
  const el = document.createElement('textarea');
  document.body.appendChild(el);
  const events: { paused: number; resumed: number; changes: string[] } = {
    paused: 0,
    resumed: 0,
    changes: [],
  };
  const unbind = bindTextarea(el, text, {
    origin: LOCAL,
    pauseRemote: () => events.paused++,
    resumeRemote: () => events.resumed++,
    onChange: (v) => events.changes.push(v),
  });
  return { doc, text, el, events, unbind };
}

function type(el: HTMLTextAreaElement, value: string, cursor = value.length) {
  el.value = value;
  el.setSelectionRange(cursor, cursor);
  el.dispatchEvent(new Event('input', { bubbles: true }));
}

describe('bindTextarea', () => {
  test('shows the document and turns typing into minimal text ops', () => {
    const { text, el, events, unbind } = setup('hello');
    expect(el.value).toBe('hello');
    type(el, 'hello world');
    expect(text.toString()).toBe('hello world');
    type(el, 'hallo world');
    expect(text.toString()).toBe('hallo world');
    expect(events.changes).toEqual(['hello world', 'hallo world']);
    unbind();
    type(el, 'gone');
    expect(text.toString()).toBe('hallo world');
  });

  test('a remote edit is drawn and keeps the cursor in place', () => {
    const { doc, text, el } = setup('hello world');
    el.focus();
    el.setSelectionRange(11, 11); // 끝
    doc.transact(() => text.insert(0, '>> '), 'remote');
    expect(el.value).toBe('>> hello world');
    expect(el.selectionStart).toBe(14);
    el.setSelectionRange(3, 3); // "hello" 앞
    doc.transact(() => text.insert(14, '!'), 'remote');
    expect(el.value).toBe('>> hello world!');
    expect(el.selectionStart).toBe(3);
  });

  test('composition pauses remote application', () => {
    const { el, events } = setup('');
    el.dispatchEvent(new Event('compositionstart'));
    expect(events.paused).toBe(1);
    el.dispatchEvent(new Event('compositionend'));
    expect(events.resumed).toBe(1);
  });

  test('local ops do not echo back through the observer', () => {
    const { el, events } = setup('a');
    type(el, 'ab');
    // onChange 는 input 경로에서 한 번만 — observer 가 자기 편집을 다시 그리지 않는다.
    expect(events.changes).toEqual(['ab']);
  });
});
