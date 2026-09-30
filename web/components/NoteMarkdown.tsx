import { type MdBlock, mdBlocks, noteInlineTokens } from '../lib';

/** 한 줄 안쪽 — **굵게** · *기울임* · `코드` · [글자](주소) · 주소. 링크는 새 탭으로 열고, 누른 것이 편집 시작으로 번지지 않게 막는다. */
function Inline({ text }: { text: string }) {
  return (
    <>
      {noteInlineTokens(text).map((token, i) => {
        const key = `${i}-${token.value}`;
        if (token.type === 'bold') {
          return (
            <strong key={key} className="font-semibold text-text">
              {token.value}
            </strong>
          );
        }
        if (token.type === 'code') {
          return (
            <code key={key} className="rounded bg-surface-2 px-1 font-mono text-[0.92em]">
              {token.value}
            </code>
          );
        }
        if (token.type === 'em') {
          return <em key={key}>{token.value}</em>;
        }
        if (token.type === 'link' || token.type === 'anchor') {
          const href = token.type === 'anchor' ? token.href : token.value;
          return (
            <a
              key={key}
              href={href}
              target="_blank"
              rel="noreferrer"
              className="text-link underline"
              onClick={(e) => e.stopPropagation()}
            >
              {token.value}
            </a>
          );
        }
        return <span key={key}>{token.value}</span>;
      })}
    </>
  );
}

function Block({ block }: { block: MdBlock }) {
  const indent = 'depth' in block ? { paddingLeft: `${block.depth * 1.25}rem` } : undefined;
  switch (block.type) {
    case 'heading':
      return (
        <p
          className={`font-bold text-text ${block.level === 1 ? 'text-title' : block.level === 2 ? 'text-body' : ''}`}
        >
          <Inline text={block.text} />
        </p>
      );
    case 'bullet':
      return (
        <p className="flex gap-2" style={indent}>
          <span aria-hidden className="text-faint">
            •
          </span>
          <span className="min-w-0">
            <Inline text={block.text} />
          </span>
        </p>
      );
    case 'task':
      return (
        <p className="flex gap-2" style={indent}>
          <span aria-hidden className={block.checked ? 'text-run' : 'text-faint'}>
            {block.checked ? '☑' : '☐'}
          </span>
          <span className={`min-w-0 ${block.checked ? 'text-faint line-through' : ''}`}>
            <Inline text={block.text} />
          </span>
        </p>
      );
    case 'ordered':
      return (
        <p className="flex gap-2" style={indent}>
          <span className="font-mono text-faint">{block.marker}</span>
          <span className="min-w-0">
            <Inline text={block.text} />
          </span>
        </p>
      );
    case 'quote':
      return (
        <p className="border-l-2 border-line pl-2 italic">
          <Inline text={block.text} />
        </p>
      );
    case 'code':
      return (
        <pre className="overflow-x-auto rounded bg-surface-2 px-2 py-1 font-mono text-meta">
          {block.text}
        </pre>
      );
    case 'rule':
      return <hr className="my-1 border-line" />;
    case 'blank':
      return <div className="h-[0.6em]" />;
    default:
      return (
        <p>
          <Inline text={block.text} />
        </p>
      );
  }
}

/** 노트 본문 미리보기 — 편집하지 않을 때 보이는 모양. 편집은 CodeMirror(`codemirror-editor.ts`). */
export function NoteMarkdown({ text }: { text: string }) {
  return (
    <div className="note-md flex flex-col gap-0.5 break-words">
      {mdBlocks(text).map((block, i) => (
        // biome-ignore lint/suspicious/noArrayIndexKey: 본문 줄 순서가 곧 정체다
        <Block key={i} block={block} />
      ))}
    </div>
  );
}
