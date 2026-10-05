import { useUiStore } from '../store';

/**
 * 맨 아래 한 줄 — 지금 도는 데몬의 버전. 데몬을 재시작할지(`rocky daemon restart`) 판단하는
 * 근거다. 이 화면을 연 뒤 데몬 버전이 바뀌었으면 화면의 번들이 옛것이라 새로고침을 권한다.
 * 버전을 모르면(구버전 데몬·health 실패) 아무것도 그리지 않는다. 좁은 창(문서 스크롤)에서도 바닥에 붙어 있다.
 */
export function VersionFooter() {
  const version = useUiStore((s) => s.daemonVersion);
  const changed = useUiStore((s) => s.daemonVersionChanged);
  if (version === null) {
    return null;
  }
  return (
    <footer className="sticky bottom-0 z-20 flex min-h-7 items-center gap-2 border-t border-line bg-surface px-4 font-mono text-chip text-faint tabular-nums">
      <span>rocky v{version}</span>
      {changed ? (
        <button
          type="button"
          className="text-mine underline underline-offset-2"
          onClick={() => window.location.reload()}
        >
          데몬 버전이 바뀌었어요 · 새로고침
        </button>
      ) : null}
    </footer>
  );
}
