import type { ReactNode } from 'react';

export interface TaskRowProps {
  /** 작업 제목 */
  title: string;
  /** 고유 번호 또는 ref (예: '#102', 'rocky-102') */
  refNumber?: string | number;
  /** 완료 여부 */
  done?: boolean;
  /** 현재 선택된 행 여부 (PC 2열 뷰에서 활성화 표시) */
  selected?: boolean;
  /** 메타 뱃지들 (우선순위, 라벨 등) */
  badges?: ReactNode;
  /** 우측 메타 정보 (예: '방금', '3분 전') */
  timeAgo?: string;
  /** 체크박스 클릭 콜백 */
  onToggleDone?: () => void;
  /** 행 클릭 콜백 (상세 열기) */
  onClick?: () => void;
  /** 정렬 핸들 등의 좌측 추가 컨트롤 */
  leading?: ReactNode;
  className?: string;
}

/**
 * Linear 스타일 작업 행 컴포넌트 — 일관된 간격, 명확한 타이포그래피, 마이크로 뱃지.
 */
export function TaskRow({
  title,
  refNumber,
  done = false,
  selected = false,
  badges,
  timeAgo,
  onToggleDone,
  onClick,
  leading,
  className = '',
}: TaskRowProps) {
  return (
    <div
      className={`group flex items-start gap-2.5 rounded-lg px-2.5 py-2 text-left transition-colors duration-150 select-none ${
        selected ? 'bg-surface-2 font-medium shadow-xs ring-1 ring-line' : 'hover:bg-surface-2/70'
      } ${done ? 'opacity-40' : ''} ${className}`}
    >
      {leading}

      {/* 미니멀 체크박스 */}
      <label className="mt-0.5 flex size-4.5 shrink-0 cursor-pointer items-center justify-center">
        <input
          type="checkbox"
          checked={done}
          aria-label={done ? '다시 열기' : '완료로 표시'}
          className="size-4 rounded-[4px] border-faint/60 text-run accent-run transition-colors focus:ring-0"
          onChange={() => onToggleDone?.()}
        />
      </label>

      {/* 본문 (1행: 제목 + ref / 2행: 태그 + 시각) — 클릭 시 상세 열기 */}
      <button
        type="button"
        className="min-w-0 flex-1 border-0 bg-transparent p-0 text-left"
        onClick={onClick}
      >
        <div className="flex items-baseline justify-between gap-2">
          <span
            className={`text-[13.5px] leading-snug break-words ${
              done ? 'text-faint line-through' : 'text-text group-hover:text-warm'
            }`}
          >
            {title}
          </span>
          {refNumber ? (
            <span className="shrink-0 font-mono text-chip tabular-nums text-faint">
              {typeof refNumber === 'number' ? `#${refNumber}` : refNumber}
            </span>
          ) : null}
        </div>

        <div className="mt-1 flex flex-wrap items-center gap-1.5 text-chip">
          {badges}
          {timeAgo ? (
            <span className="ml-auto font-mono text-chip tabular-nums text-faint">{timeAgo}</span>
          ) : null}
        </div>
      </button>
    </div>
  );
}
