import type { EditorView } from '@codemirror/view';
import { applyFormat, FORMAT_ACTIONS } from '../markdown-commands';
import { FORMAT_ICONS } from './format-icons';

/**
 * 마크다운 서식 툴바 — 노트 본문과 할 일 설명이 같이 쓴다. 누르는 순간 편집기가 포커스를 잃으면 선택이
 * 풀리므로 포커스를 뺏지 않는다(`onMouseDown` preventDefault). 위치·배경은 놓이는 자리가 `className` 으로.
 */
export function FormatToolbar(props: {
  view: () => EditorView | null | undefined;
  className?: string;
  onFormat?: (actionId: string) => void;
}) {
  const { view, className = '', onFormat } = props;
  return (
    <div className={`flex flex-wrap gap-0.5 ${className}`} role="toolbar" aria-label="서식">
      {FORMAT_ACTIONS.map((action) => {
        const Icon = FORMAT_ICONS[action.id];
        const hint = action.shortcut ? `${action.label} (${action.shortcut})` : action.label;
        return (
          <button
            key={action.id}
            type="button"
            className="flex size-7 items-center justify-center rounded text-muted hover:bg-surface-2 hover:text-text"
            title={hint}
            aria-label={action.label}
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => {
              const target = view();
              if (target) {
                onFormat?.(action.id);
                applyFormat(target, action.run);
              }
            }}
          >
            {Icon ? <Icon size={15} aria-hidden /> : action.label}
          </button>
        );
      })}
    </div>
  );
}
