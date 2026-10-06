import { useState } from 'react';
import type { SpawnResult, TodoView } from '../../types';
import { useUiStore } from '../../store';

export function SpawnAction({ todo }: { todo: TodoView }) {
  const boards = useUiStore((s) => s.boards);
  const spawnAllowed = useUiStore((s) => s.spawnAllowed);
  const spawnSession = useUiStore((s) => s.spawnSession);
  const [path, setPath] = useState('');
  const [note, setNote] = useState('');
  const [asking, setAsking] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<SpawnResult | null>(null);

  const board = boards.find((b) => b.id === todo.boardId);

  if (!spawnAllowed) {
    return (
      <div className="mt-2.5 flex flex-col gap-1.5">
        <p className="m-0 text-meta leading-[1.4] text-muted">
          로컬(루프백) 주소로 연 화면에서만 세션을 띄울 수 있어요. 이 화면은 외부에 노출된 데몬을
          거쳐 열렸어요.
        </p>
      </div>
    );
  }

  const submit = async (): Promise<void> => {
    setError(null);
    setBusy(true);
    try {
      setResult(
        await spawnSession(todo.id, {
          note: note.trim() || undefined,
          path: asking ? path.trim() : undefined,
        }),
      );
      setAsking(false);
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      setError(message);
      // 경로 문제면 입력을 (다시) 열어 고칠 값을 보여준다 — 실패가 조용히 막다른 길이 되면 안 된다. rc 서버 · 세션 실패는
      // 경로와 무관하니 사유만 보인다.
      if (!board?.path || /경로|git 워크트리가 아니다:/.test(message)) {
        setAsking(true);
        setPath(path || board?.path || '');
      }
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="mt-2.5 flex flex-col gap-1.5">
      {asking && (
        <input
          className="w-full rounded-md border border-line bg-bg px-2 py-1.5 text-inherit"
          value={path}
          placeholder="/Users/…/레포 절대경로"
          aria-label="메인 레포 절대경로"
          onChange={(e) => setPath(e.target.value)}
        />
      )}
      <input
        className="w-full rounded-md border border-line bg-bg px-2 py-1.5 text-inherit"
        value={note}
        placeholder="메모 (선택)"
        aria-label="세션에 함께 보낼 메모"
        onChange={(e) => setNote(e.target.value)}
      />
      <div className="drawer-actions">
        <button
          type="button"
          className="drawer-btn"
          disabled={busy || (asking && path.trim() === '')}
          onClick={() => {
            if (!board?.path && !asking) {
              setAsking(true);
              setPath(board?.path ?? '');
              return;
            }
            void submit();
          }}
        >
          {busy ? '띄우는 중… (보통 몇십 초, 길면 몇 분)' : '새 세션 띄우기'}
        </button>
      </div>
      {result && (
        <div className="mt-1.5 flex flex-col gap-1 text-meta leading-[1.4] text-handoff [&_code]:select-all">
          {result.reused ? (
            <span>이미 실행 중인 세션에 넘겼어요 · {result.worktreePath}</span>
          ) : result.server ? (
            <span>
              원격 제어 서버 “{result.server.name}” 의 세션에 넘겼어요 — 폰 · 웹의 원격 제어
              목록에서 이어 볼 수 있어요. 끝나면 서버는 직접 닫아요 · {result.worktreePath}
            </span>
          ) : (
            <>
              <span>
                세션 {result.sessionShortId} · {result.worktreePath}
              </span>
              <code>claude attach {result.sessionShortId}</code>
              {result.warning && <span className="text-p1">⚠ {result.warning}</span>}
            </>
          )}
          {result.woke === false && <span>깨우지 못했어요 — 그 세션의 다음 턴에 집어요</span>}
        </div>
      )}
      {/* 실패 사유는 즉시 읽혀야 한다 — 보이기만 하면 스크린리더가 놓친다. */}
      {error && (
        <div className="mt-1.5 whitespace-pre-wrap text-meta leading-[1.4] text-p1" role="alert">
          {error}
        </div>
      )}
    </div>
  );
}
