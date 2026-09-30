import {
  Bold,
  Code,
  Heading2,
  Italic,
  Link,
  List,
  ListChecks,
  type LucideIcon,
  TextQuote,
} from 'lucide-react';

/** 서식 툴바 아이콘 — `FORMAT_ACTIONS` 의 id 로 찾는다(명령은 아이콘을 모른다). */
export const FORMAT_ICONS: Record<string, LucideIcon> = {
  h2: Heading2,
  bold: Bold,
  italic: Italic,
  code: Code,
  link: Link,
  list: List,
  task: ListChecks,
  quote: TextQuote,
};
