import { X } from 'lucide-react';

/** 숨기기 버튼 — 24px 이상 누르는 자리. */
export function HideButton(props: { label: string; onClick: () => void }) {
  return (
    <button
      type="button"
      className="m-1 flex size-7 shrink-0 items-center justify-center rounded-md text-faint hover:bg-surface-2 hover:text-text"
      aria-label={props.label}
      title="숨기기 — 상태가 바뀌면 다시 보인다"
      onClick={props.onClick}
    >
      <X size={14} aria-hidden />
    </button>
  );
}
