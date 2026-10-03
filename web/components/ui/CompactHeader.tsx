import type { ReactNode } from 'react';

export interface CompactHeaderProps {
  /** 좌측 보드 스위처 영역 */
  leftSlot: ReactNode;
  /** 중앙 세그먼트 탭 영역 */
  centerSlot: ReactNode;
  /** 우측 상태 뱃지 및 액션 버튼 영역 */
  rightSlot?: ReactNode;
  className?: string;
}

/**
 * 42px 초슬림 통합 헤더 — 기존의 3단 적층(TopBar + NowTable + ViewSwitch)을
 * 세련된 한 줄로 통합하여 세로 뷰포트를 극대화합니다.
 */
export function CompactHeader({
  leftSlot,
  centerSlot,
  rightSlot,
  className = '',
}: CompactHeaderProps) {
  return (
    <header
      className={`sticky top-0 z-20 flex min-h-10 items-center justify-between gap-2 border-b border-line bg-surface/95 px-3 py-1.5 backdrop-blur-xs select-none ${className}`}
    >
      <div className="flex shrink-0 items-center gap-2">{leftSlot}</div>
      <div className="flex min-w-0 items-center justify-center">{centerSlot}</div>
      <div className="flex shrink-0 items-center gap-1.5">{rightSlot}</div>
    </header>
  );
}
