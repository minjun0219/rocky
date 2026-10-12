import { afterEach, describe, expect, test } from 'bun:test';
import { cleanup, screen } from '@testing-library/react';
import { useUiStore } from '../store';
import { renderWithStore } from '../test-support';
import { VersionFooter } from './VersionFooter';

afterEach(cleanup);

describe('VersionFooter', () => {
  test('데몬 버전을 보여 준다', () => {
    renderWithStore(<VersionFooter />, {
      daemonVersion: '0.33.1',
      daemonCommit: null,
      daemonVersionChanged: false,
    });
    expect(screen.getByText('rocky v0.33.1')).toBeTruthy();
    expect(screen.queryByRole('button')).toBeNull();
  });

  test('빌드한 커밋을 알면 버전 오른쪽 괄호에 붙인다', () => {
    renderWithStore(<VersionFooter />, {
      daemonVersion: '0.49.0',
      daemonCommit: 'abc1234',
      daemonVersionChanged: false,
    });
    expect(screen.getByText('rocky v0.49.0 (abc1234)')).toBeTruthy();
  });

  test('버전을 모르면 그리지 않는다', () => {
    const { container } = renderWithStore(<VersionFooter />, { daemonVersion: null });
    expect(container.textContent).toBe('');
  });

  test('화면을 연 뒤 데몬이 바뀌었으면 새로고침을 권한다', () => {
    renderWithStore(<VersionFooter />, { daemonVersion: '0.34.0', daemonVersionChanged: true });
    expect(
      screen.getByRole('button', { name: '데몬이 새 빌드로 바뀌었어요 · 새로고침' }),
    ).toBeTruthy();
  });
});

describe('loadCapabilities — 데몬 버전 추적', () => {
  const realFetch = globalThis.fetch;
  afterEach(() => {
    globalThis.fetch = realFetch;
  });
  const serve = (version: string, commit?: string) => {
    globalThis.fetch = (async () =>
      new Response(JSON.stringify({ version, commit }), {
        status: 200,
      })) as unknown as typeof fetch;
  };

  test('재연결에서 버전이 달라지면 바뀐 것으로 남는다 — 되돌아가도 풀리지 않는다', async () => {
    useUiStore.setState({ daemonVersion: null, daemonCommit: null, daemonVersionChanged: false });
    serve('0.33.1');
    await useUiStore.getState().loadCapabilities();
    expect(useUiStore.getState().daemonVersionChanged).toBe(false);
    serve('0.34.0');
    await useUiStore.getState().loadCapabilities();
    expect(useUiStore.getState().daemonVersion).toBe('0.34.0');
    expect(useUiStore.getState().daemonVersionChanged).toBe(true);
    serve('0.33.1');
    await useUiStore.getState().loadCapabilities();
    expect(useUiStore.getState().daemonVersionChanged).toBe(true);
  });

  test('버전이 같아도 커밋이 달라지면 바뀐 것이다 — main 빌드끼리', async () => {
    useUiStore.setState({ daemonVersion: null, daemonCommit: null, daemonVersionChanged: false });
    serve('0.49.0', 'abc1234');
    await useUiStore.getState().loadCapabilities();
    expect(useUiStore.getState().daemonCommit).toBe('abc1234');
    serve('0.49.0', 'abc1234');
    await useUiStore.getState().loadCapabilities();
    expect(useUiStore.getState().daemonVersionChanged).toBe(false);
    serve('0.49.0', 'def5678');
    await useUiStore.getState().loadCapabilities();
    expect(useUiStore.getState().daemonVersionChanged).toBe(true);
  });
});
