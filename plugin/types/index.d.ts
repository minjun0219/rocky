/** rocky lab band 한 줄의 재료 — 턴 경계에서 데몬을 읽어 쓰고, AbovePrompt 가 읽는다. */
export type LabBand = {
  daemon: 'ok' | 'down';
  /** 이 세션 cwd 의 보드 key(summary 가 고른 것, 없으면 받은편지함 등록의 것). null 이면 전체 합계다. */
  board: string | null;
  doing: number;
  collect: number;
  handoffsOpen: number;
  overdue: number;
  /** 데몬 받은편지함에 이 세션 id 가 등록돼 있나 — 없으면 PR 알림이 이 세션에 못 온다. 못 읽었으면 null. */
  registered: boolean | null;
  /** 이 세션이 구독한 PR 수. */
  watching: number;
  /** 마지막으로 받은 rocky 메시지의 toast 줄. */
  last: string | null;
  /** 데몬을 못 읽었을 때 그 사유(경로 → 상태·에러). */
  error: string | null;
};

declare module 'claude-code' {
  interface PluginState {
    rocky: { labBand: LabBand | null };
  }
}
