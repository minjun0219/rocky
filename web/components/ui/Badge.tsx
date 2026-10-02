import type { ReactNode } from 'react';

export type BadgeTone = 'run' | 'mine' | 'dead' | 'prio' | 'agent' | 'neutral';

export interface BadgeProps {
  /** 뱃지의 색상 톤 (상태에만 색을 쓰는 관제판 규칙) */
  tone?: BadgeTone;
  /** 내용 텍스트 또는 자식 요소 */
  children: ReactNode;
  /** 추가 클래스 */
  className?: string;
  /** 라벨/툴팁 */
  title?: string;
}

const TONE_CLASSES: Record<BadgeTone, string> = {
  // 돌고 있음 (세션 실행 중)
  run: 'text-run bg-run-soft border-run/20',
  // 내 차례 (사람이 손댈 것)
  mine: 'text-mine bg-mine-soft border-mine/25 font-semibold',
  // 세션 없음 / 에러
  dead: 'text-dead bg-dead-soft border-dead/25',
  // 긴급 우선순위 (P1/P2 등)
  prio: 'text-mine bg-mine-soft border-mine/30 font-semibold',
  // 에이전트 표시
  agent: 'text-run bg-run-soft border-run/25 font-medium',
  // 일반 메타 라벨
  neutral: 'text-muted bg-surface-2 border-line',
};

/**
 * 모던 마이크로 뱃지 — 4px 모서리, 11px~12px 모노스페이스 숫자/태그.
 * 999px 알약형(pill) 대신 각진 4px 모서리를 사용하여 일관된 Linear 스타일을 구현합니다.
 */
export function Badge({ tone = 'neutral', children, className = '', title }: BadgeProps) {
  return (
    <span
      className={`inline-flex items-center gap-1 rounded-[4px] border px-1.5 py-0.5 font-mono text-chip leading-none tabular-nums ${TONE_CLASSES[tone]} ${className}`}
      title={title}
    >
      {children}
    </span>
  );
}
