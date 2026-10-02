export interface LiveAgentCardProps {
  /** 작업 제목 */
  title: string;
  /** 에이전트/세션 이름 */
  actor?: string;
  /** 경과 시간 문자열 (예: '04:12', '12분') */
  elapsed: string;
  /** 클릭 시 호출될 콜백 */
  onClick?: () => void;
  className?: string;
}

/**
 * 실시간 진행 중 에이전트 작업 카드 — 점멸 펄스 점과 경과 시간을 표시합니다.
 */
export function LiveAgentCard({
  title,
  actor,
  elapsed,
  onClick,
  className = '',
}: LiveAgentCardProps) {
  return (
    <button
      type="button"
      className={`group flex w-full items-center justify-between gap-3 rounded-lg border border-line bg-surface px-3 py-2.5 text-left transition-colors duration-150 hover:border-run/40 hover:bg-surface-2 focus-visible:outline-2 focus-visible:outline-warm ${className}`}
      onClick={onClick}
    >
      <div className="flex min-w-0 items-center gap-2.5">
        {/* 실시간 펄스 인디케이터 */}
        <span className="relative flex size-2 shrink-0">
          <span className="absolute inline-flex size-full animate-ping rounded-full bg-run opacity-75" />
          <span className="relative inline-flex size-2 rounded-full bg-run" />
        </span>
        <div className="min-w-0 flex-1">
          <p className="truncate text-sm font-medium text-text group-hover:text-run">{title}</p>
          {actor ? <p className="font-mono text-chip text-muted">{actor}</p> : null}
        </div>
      </div>
      <span className="shrink-0 font-mono text-chip font-semibold tabular-nums text-run">
        {elapsed}
      </span>
    </button>
  );
}
