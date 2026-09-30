/**
 * 마크다운 보기 — 할 일 설명 · 댓글 · 노트 본문이 모두 이것 하나로 그려진다.
 *
 * 파서는 노트 편집기(CodeMirror `lang-markdown`)가 이미 번들에 싣고 있는 `@lezer/markdown`(GFM)이다 —
 * 새 의존이 없고, 편집기 하이라이트와 미리보기가 같은 문법으로 읽는다. 예전의 줄 단위 정규식은
 * 본문 262건 중 62건에서 강조·코드·링크를 깨뜨렸다(굵게 안의 코드, 줄을 넘는 `**`, `[글자](주소)`
 * 의 `)` 가 주소에 붙음). GFM 파서로는 1건(한국어 조사 바로 앞에서 닫히는 `**`)만 남는다.
 *
 * HTML 문자열을 만들지 않고 React 요소로 그린다 — 본문은 에이전트와 외부 수집함이 쓴 글이라
 * `innerHTML` 을 쓰지 않는다. 링크는 http(s)·mailto 만 걸고, 이미지는 불러오지 않고 링크로 둔다.
 */
import type { SyntaxNode } from '@lezer/common';
import { GFM, parser } from '@lezer/markdown';
import type { ReactNode } from 'react';

const md = parser.configure(GFM);

/** 글자로 보이지 않는 표지 — 모양은 요소가 대신 낸다. */
const MARKS = new Set([
  'HeaderMark',
  'ListMark',
  'QuoteMark',
  'EmphasisMark',
  'CodeMark',
  'CodeInfo',
  'LinkMark',
  'StrikethroughMark',
  'TaskMarker',
  'TableDelimiter',
  'LinkTitle',
  'LinkLabel',
]);

const ENTITIES: Record<string, string> = {
  '&amp;': '&',
  '&lt;': '<',
  '&gt;': '>',
  '&quot;': '"',
  '&#39;': "'",
  '&nbsp;': ' ',
};

/** 걸어도 되는 주소인가 — `javascript:` 같은 스킴은 글자로만 둔다. */
export function safeHref(url: string): string | null {
  const trimmed = url.trim();
  if (/^(https?:|mailto:)/i.test(trimmed)) {
    return trimmed;
  }
  if (/^www\./i.test(trimmed)) {
    return `https://${trimmed}`;
  }
  return null;
}

function children(node: SyntaxNode): SyntaxNode[] {
  const out: SyntaxNode[] = [];
  for (let c = node.firstChild; c; c = c.nextSibling) {
    out.push(c);
  }
  return out;
}

function Anchor({ href, children: label }: { href: string; children: ReactNode }) {
  return (
    <a
      href={href}
      target="_blank"
      rel="noreferrer"
      // 미리보기는 누르면 편집이 열리는 자리이기도 하다 — 링크를 누른 것이 거기까지 번지지 않게.
      onClick={(e) => e.stopPropagation()}
    >
      {label}
    </a>
  );
}

/** 한 블록 안의 인라인을 그린다. 문단 안의 줄바꿈은 `<br>` — 메모·댓글은 줄을 나눈 대로 읽힌다. */
class Inline {
  private key = 0;
  /** 바로 앞에서 줄이 바뀌었나(블록 첫머리 포함) — 제목의 `# ` 뒤, 인용의 `> ` 뒤 공백을 걷는다. */
  private afterBreak = true;

  constructor(private readonly src: string) {}

  range(parent: SyntaxNode, from: number, to: number): ReactNode[] {
    const out: ReactNode[] = [];
    let pos = from;
    for (const child of children(parent)) {
      if (child.to <= from || child.from >= to) {
        continue;
      }
      if (child.from > pos) {
        this.text(this.src.slice(pos, child.from), out);
      }
      const before = out.length;
      this.node(child, out);
      // 무언가 그렸으면(코드·강조·링크 …) 줄 첫머리가 아니다 — 뒤 글의 앞 공백을 지킨다.
      // 표지(`#`·`>`·`**`)만 건너뛴 경우와 줄바꿈은 그대로 둔다.
      const last = out[out.length - 1];
      const isBreak =
        typeof last === 'object' && last !== null && (last as { type?: unknown }).type === 'br';
      if (out.length > before && !isBreak) {
        this.afterBreak = false;
      }
      pos = child.to;
    }
    if (to > pos) {
      this.text(this.src.slice(pos, to), out);
    }
    return out;
  }

  all(node: SyntaxNode): ReactNode[] {
    return this.range(node, node.from, node.to);
  }

  private text(raw: string, out: ReactNode[]) {
    const lines = raw.split('\n');
    lines.forEach((line, i) => {
      if (i > 0) {
        out.push(<br key={this.key++} />);
        this.afterBreak = true;
      }
      const piece = this.afterBreak ? line.replace(/^[ \t]+/, '') : line;
      if (piece) {
        out.push(piece);
        this.afterBreak = false;
      }
    });
  }

  private node(n: SyntaxNode, out: ReactNode[]) {
    const src = this.src;
    const key = this.key++;
    if (MARKS.has(n.name)) {
      return;
    }
    switch (n.name) {
      case 'Emphasis':
        out.push(<em key={key}>{this.all(n)}</em>);
        return;
      case 'StrongEmphasis':
        out.push(<strong key={key}>{this.all(n)}</strong>);
        return;
      case 'Strikethrough':
        out.push(<del key={key}>{this.all(n)}</del>);
        return;
      case 'InlineCode': {
        const marks = children(n).filter((c) => c.name === 'CodeMark');
        const from = marks[0]?.to ?? n.from;
        const to = marks.length > 1 ? (marks[marks.length - 1]?.from ?? n.to) : n.to;
        out.push(<code key={key}>{src.slice(from, to)}</code>);
        return;
      }
      case 'Link':
      case 'Image': {
        const marks = children(n).filter((c) => c.name === 'LinkMark');
        const url = children(n).find((c) => c.name === 'URL');
        const labelFrom = marks[0]?.to ?? n.from;
        const labelTo = marks[1]?.from ?? n.to;
        const label = this.range(n, labelFrom, labelTo);
        const href = url ? safeHref(src.slice(url.from, url.to)) : null;
        if (!href) {
          out.push(<span key={key}>{label}</span>);
          return;
        }
        // 이미지는 불러오지 않는다 — 외부 주소로 요청이 나가고, 좁은 패널에서 자리를 먹는다.
        out.push(
          <Anchor key={key} href={href}>
            {n.name === 'Image' ? <>🖼 {label}</> : label}
          </Anchor>,
        );
        return;
      }
      case 'Autolink':
      case 'URL': {
        const urlNode = n.name === 'URL' ? n : children(n).find((c) => c.name === 'URL');
        const text = urlNode ? src.slice(urlNode.from, urlNode.to) : src.slice(n.from, n.to);
        const href = safeHref(text);
        out.push(
          href ? (
            <Anchor key={key} href={href}>
              {text}
            </Anchor>
          ) : (
            text
          ),
        );
        return;
      }
      case 'Escape':
        out.push(src.slice(n.from + 1, n.to));
        return;
      case 'Entity': {
        const raw = src.slice(n.from, n.to);
        out.push(ENTITIES[raw] ?? raw);
        return;
      }
      case 'HardBreak':
        out.push(<br key={key} />);
        this.afterBreak = true;
        return;
      case 'HTMLTag': {
        const raw = src.slice(n.from, n.to);
        // 에이전트가 줄바꿈 대신 쓰는 `<br>` 만 살린다 — 나머지 태그는 글자로 보인다(React 가 이스케이프).
        out.push(/^<br\s*\/?>$/i.test(raw) ? <br key={key} /> : raw);
        return;
      }
      default:
        this.text(src.slice(n.from, n.to), out);
    }
  }
}

function inline(src: string, node: SyntaxNode, from = node.from, to = node.to): ReactNode[] {
  return new Inline(src).range(node, from, to);
}

function listItem(src: string, item: SyntaxNode, key: number): ReactNode {
  const task = children(item).find((c) => c.name === 'Task');
  if (task) {
    const marker = children(task).find((c) => c.name === 'TaskMarker');
    const checked = marker ? /x/i.test(src.slice(marker.from, marker.to)) : false;
    return (
      <li key={key} className={`md-task ${checked ? 'is-done' : ''}`}>
        <span aria-hidden className="md-task-box">
          {checked ? '☑' : '☐'}
        </span>{' '}
        <span className="md-task-text">{inline(src, task, marker?.to ?? task.from)}</span>
        {blocks(src, item, (c) => c.name !== 'Task' && c.name !== 'ListMark')}
      </li>
    );
  }
  return <li key={key}>{blocks(src, item, (c) => c.name !== 'ListMark')}</li>;
}

function codeBody(src: string, node: SyntaxNode): string {
  if (node.name === 'CodeBlock') {
    // 들여쓴 코드 — 네 칸을 걷는다.
    return src
      .slice(node.from, node.to)
      .split('\n')
      .map((line) => line.replace(/^( {1,4}|\t)/, ''))
      .join('\n');
  }
  const texts = children(node).filter((c) => c.name === 'CodeText');
  if (texts.length === 0) {
    return '';
  }
  return src.slice(texts[0]?.from ?? node.from, texts[texts.length - 1]?.to ?? node.to);
}

function table(src: string, node: SyntaxNode, key: number): ReactNode {
  const cells = (row: SyntaxNode, Tag: 'th' | 'td') =>
    children(row)
      .filter((c) => c.name === 'TableCell')
      .map((cell, i) => (
        // biome-ignore lint/suspicious/noArrayIndexKey: 칸 순서가 곧 정체다
        <Tag key={i}>{inline(src, cell)}</Tag>
      ));
  const header = children(node).find((c) => c.name === 'TableHeader');
  const rows = children(node).filter((c) => c.name === 'TableRow');
  return (
    <div key={key} className="md-table">
      <table>
        {header && (
          <thead>
            <tr>{cells(header, 'th')}</tr>
          </thead>
        )}
        <tbody>
          {rows.map((row, i) => (
            // biome-ignore lint/suspicious/noArrayIndexKey: 행 순서가 곧 정체다
            <tr key={i}>{cells(row, 'td')}</tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function block(src: string, n: SyntaxNode, key: number): ReactNode {
  const heading = n.name.match(/^(?:ATX|Setext)Heading(\d)$/);
  if (heading) {
    const level = Number(heading[1]);
    const Tag = `h${Math.min(level, 6)}` as 'h1';
    // Setext(`제목⏎===`)는 밑줄 앞 줄바꿈까지만 — 안 그러면 끝에 빈 줄이 붙는다.
    const underline = n.name.startsWith('Setext') ? n.lastChild : null;
    const to = underline?.name === 'HeaderMark' ? underline.from : n.to;
    return (
      <Tag key={key}>
        {inline(src, n, n.from, to).filter(
          (c, i, a) =>
            !(
              i === a.length - 1 &&
              typeof c === 'object' &&
              c !== null &&
              (c as { type?: unknown }).type === 'br'
            ),
        )}
      </Tag>
    );
  }
  switch (n.name) {
    case 'Paragraph':
      return <p key={key}>{inline(src, n)}</p>;
    case 'BulletList':
      return <ul key={key}>{children(n).map((item, i) => listItem(src, item, i))}</ul>;
    case 'OrderedList': {
      const first = children(children(n)[0] ?? n).find((c) => c.name === 'ListMark');
      const start = first ? Number.parseInt(src.slice(first.from, first.to), 10) : 1;
      return (
        <ol key={key} start={Number.isFinite(start) ? start : 1}>
          {children(n).map((item, i) => listItem(src, item, i))}
        </ol>
      );
    }
    case 'Blockquote':
      return <blockquote key={key}>{blocks(src, n, (c) => c.name !== 'QuoteMark')}</blockquote>;
    case 'FencedCode':
    case 'CodeBlock':
      return (
        <pre key={key}>
          <code>{codeBody(src, n)}</code>
        </pre>
      );
    case 'HorizontalRule':
      return <hr key={key} />;
    case 'Table':
      return table(src, n, key);
    case 'LinkReference':
      return null;
    default:
      // HTML 블록·주석 등 — 글자 그대로(React 가 이스케이프한다).
      return <p key={key}>{src.slice(n.from, n.to)}</p>;
  }
}

function blocks(src: string, parent: SyntaxNode, keep: (n: SyntaxNode) => boolean = () => true) {
  return children(parent)
    .filter(keep)
    .map((n, i) => block(src, n, i));
}

/** 본문을 그린다. 스타일은 `.md` 아래(`web/styles/markdown.css`). */
export function Markdown({ text, className = '' }: { text: string; className?: string }) {
  const tree = md.parse(text);
  return <div className={`md ${className}`}>{blocks(text, tree.topNode)}</div>;
}

/**
 * 한 줄 요약 — 노트 목록 행의 둘째 줄. 첫 내용 블록의 글자만(기호·주소 없이) 뽑아 자른다.
 */
export function markdownExcerpt(text: string, max = 80): string {
  const tree = md.parse(text);
  const first = children(tree.topNode).find(
    (n) => n.name !== 'HorizontalRule' && n.name !== 'LinkReference',
  );
  if (!first) {
    return '';
  }
  let target: SyntaxNode = first;
  // 목록·인용이면 첫 항목의 첫 글 블록까지 내려간다.
  while (['BulletList', 'OrderedList', 'ListItem', 'Blockquote'].includes(target.name)) {
    const next = children(target).find(
      (c) => !MARKS.has(c.name) && c.name !== 'ListMark' && c.name !== 'QuoteMark',
    );
    if (!next) {
      break;
    }
    target = next;
  }
  const plain = (n: SyntaxNode): string => {
    let out = '';
    let pos = n.from;
    for (const c of children(n)) {
      out += text.slice(pos, c.from);
      if (c.name === 'URL' && n.name === 'Link') {
        // 링크 주소는 요약에 넣지 않는다
      } else if (!MARKS.has(c.name) && c.name !== 'CodeMark') {
        out += c.name === 'InlineCode' ? text.slice(c.from, c.to).replace(/`/g, '') : plain(c);
      }
      pos = c.to;
    }
    return out + text.slice(pos, n.to);
  };
  const line = (
    target.name === 'FencedCode' || target.name === 'CodeBlock'
      ? codeBody(text, target)
      : plain(target)
  )
    .split('\n')[0]!
    .trim();
  return line.length > max ? `${line.slice(0, max - 1)}…` : line;
}
