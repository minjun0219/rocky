import { afterEach, beforeEach, describe, expect, mock, test } from 'bun:test';
import { cleanup, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { renderWithStore } from '../test-support';
import { WorklogPane } from './WorklogPane';

afterEach(cleanup);
const realFetch = globalThis.fetch;
let requested: string[] = [];
function serve(body: unknown) {
  requested = [];
  globalThis.fetch = (async (input: string) => {
    requested.push(String(input));
    return new Response(JSON.stringify(body), { status: 200 });
  }) as unknown as typeof fetch;
}
beforeEach(() => serve({ entries: [] }));
afterEach(() => {
  globalThis.fetch = realFetch;
});

describe('WorklogPane — 작업로그 탭', () => {
  test('턴 기록은 요청을 제목처럼, 결과를 둘째 줄로 — 할 일 태그는 누르면 상세', async () => {
    serve({
      entries: [
        {
          id: '1',
          projectKey: 'rocky-4745b950',
          timestamp: new Date().toISOString(),
          kind: 'turn',
          content: 'req: 피드 탭 만들어 | tools: Edit(×3) | did: 피드를 첫 화면으로',
          tags: ['turn', 'todo:rocky-12'],
          todoRef: 'rocky-12',
        },
      ],
    });
    const openTodoDetail = mock(async () => {});
    renderWithStore(<WorklogPane />, { selected: 'rocky', openTodoDetail });
    expect(await screen.findByText('피드 탭 만들어')).toBeTruthy();
    expect(screen.getByText('피드를 첫 화면으로')).toBeTruthy();
    expect(requested[0]).toBe('/api/logs/worklog?limit=50&board=rocky');
    await userEvent.click(screen.getByRole('button', { name: 'rocky-12' }));
    expect(openTodoDetail).toHaveBeenCalledWith('rocky-12');
  });

  test('보드에 path 가 없으면 왜 비었는지 말한다', async () => {
    serve({ entries: [], unlinked: true });
    renderWithStore(<WorklogPane />, { selected: 'bare' });
    expect(await screen.findByText(/폴더\(path\)가 없어/)).toBeTruthy();
  });

  test('종류를 고르면 그 종류로 다시 묻는다', async () => {
    renderWithStore(<WorklogPane />, { selected: 'all' });
    await waitFor(() => expect(requested).toEqual(['/api/logs/worklog?limit=50']));
    await userEvent.selectOptions(screen.getByRole('combobox', { name: '종류' }), 'decision');
    await waitFor(() => expect(requested.at(-1)).toBe('/api/logs/worklog?limit=50&kind=decision'));
  });
});
