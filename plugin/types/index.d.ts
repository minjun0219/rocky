// rocky 플러그인의 function hooks 상태 계약 — `claude plugin validate` 가 모듈이 쓰는 `$.state` 키를 여기에 맞춰 본다.
// 계약은 import 없이 혼자 서야 해서 모양을 hooks/board-panel/data.ts 의 `BoardView` 에서 옮겨 적었다 — 어긋나면
// 플러그인 타입 검사(register.tsx 의 atom·update)가 잡는다.

export type BoardPanelItem = { ref: string; title: string; status: 'doing' | 'todo' };

export type BoardPanelView = {
  board: string | null;
  items: BoardPanelItem[];
  collect: number | null;
  error: string | null;
  at: string | null;
};

declare module 'claude-code' {
  interface PluginState {
    rocky: { boardPanel: BoardPanelView };
  }
}
