// A small, safe subset of Markdown: **bold**, *italic*, `code`, ```blocks```, > quotes and links.
// It builds a tree that the interface turns into elements; nothing is ever put together as HTML.

export type Inline =
  | { type: 'text'; text: string }
  | { type: 'bold'; children: Inline[] }
  | { type: 'italic'; children: Inline[] }
  | { type: 'code'; text: string }
  | { type: 'link'; href: string; children: Inline[] };

export type Block =
  | { type: 'paragraph'; children: Inline[] }
  | { type: 'quote'; children: Inline[] }
  | { type: 'code'; text: string; language: string };

/** Longest text parsed; the rest is shown as it is (the server allows 4000 characters). */
const MAX_PARSED = 8000;
const MAX_DEPTH = 4;

const URL_PATTERN = /\bhttps?:\/\/[^\s<>"']+/giu;
const count = (text: string, char: string) => text.split(char).length - 1;

/** Cut punctuation that belongs to the sentence, not to the address; brackets only when they have no partner. */
function trimUrl(raw: string): string {
  let url = raw;
  for (;;) {
    const last = url.at(-1);
    if (last === undefined) return url;
    const stray = '.,;:!?»”’'.includes(last)
      || (last === ')' && count(url, ')') > count(url, '('))
      || (last === ']' && count(url, ']') > count(url, '['))
      || (last === '}' && count(url, '}') > count(url, '{'));
    if (!stray) return url;
    url = url.slice(0, -1);
  }
}

/** Only web links open; `javascript:`, `data:`, `file:` and friends are plain text. */
export function safeHref(candidate: string): string | null {
  try {
    const url = new URL(candidate.trim());
    return url.protocol === 'http:' || url.protocol === 'https:' ? url.href : null;
  } catch {
    return null;
  }
}

function splitAutolinks(text: string): Inline[] {
  const parts: Inline[] = [];
  let cursor = 0;
  for (const match of text.matchAll(URL_PATTERN)) {
    const index = match.index ?? 0;
    const raw = trimUrl(match[0]);
    const href = safeHref(raw);
    if (!href) continue;
    if (index > cursor) parts.push({ type: 'text', text: text.slice(cursor, index) });
    parts.push({ type: 'link', href, children: [{ type: 'text', text: raw }] });
    cursor = index + raw.length;
  }
  if (cursor < text.length) parts.push({ type: 'text', text: text.slice(cursor) });
  return parts.length > 0 ? parts : [{ type: 'text', text }];
}

/** Find the closing `marker` after `from` that is not at the very start and not followed by a letter of the same marker. */
function closeIndex(text: string, marker: string, from: number): number {
  let i = from;
  while (i < text.length) {
    const at = text.indexOf(marker, i);
    if (at < 0) return -1;
    if (at > from && text[at - 1] !== ' ') return at;
    i = at + marker.length;
  }
  return -1;
}

export function parseInline(text: string, depth = 0): Inline[] {
  const out: Inline[] = [];
  let plain = '';
  const flush = () => {
    if (plain) {
      out.push(...splitAutolinks(plain));
      plain = '';
    }
  };
  let i = 0;
  while (i < text.length) {
    const rest = text.slice(i);
    if (rest.startsWith('`')) {
      const end = text.indexOf('`', i + 1);
      if (end > i + 1) {
        flush();
        out.push({ type: 'code', text: text.slice(i + 1, end) });
        i = end + 1;
        continue;
      }
    }
    if (depth < MAX_DEPTH && (rest.startsWith('**') || rest.startsWith('*') || rest.startsWith('_'))) {
      const marker = rest.startsWith('**') ? '**' : rest[0];
      const startsWord = i === 0 || !/[\p{L}\p{N}]/u.test(text[i - 1]);
      const afterMarker = text[i + marker.length];
      if ((marker !== '_' || startsWord) && afterMarker !== undefined && afterMarker !== ' ') {
        const end = closeIndex(text, marker, i + marker.length);
        if (end > 0) {
          flush();
          const children = parseInline(text.slice(i + marker.length, end), depth + 1);
          out.push(marker === '**' ? { type: 'bold', children } : { type: 'italic', children });
          i = end + marker.length;
          continue;
        }
      }
    }
    if (rest.startsWith('[') && depth < MAX_DEPTH) {
      const match = /^\[([^\]\n]{1,200})\]\(([^)\s]{1,2000})\)/u.exec(rest);
      const href = match ? safeHref(match[2]) : null;
      if (match && href) {
        flush();
        out.push({ type: 'link', href, children: [{ type: 'text', text: match[1] }] });
        i += match[0].length;
        continue;
      }
    }
    plain += text[i];
    i += 1;
  }
  flush();
  return out;
}

export function parseMarkdown(source: string): Block[] {
  const text = source.length > MAX_PARSED ? source.slice(0, MAX_PARSED) : source;
  const lines = text.replace(/\r\n?/g, '\n').split('\n');
  const blocks: Block[] = [];
  let paragraph: string[] = [];
  let quote: string[] = [];
  const endParagraph = () => {
    if (paragraph.length) blocks.push({ type: 'paragraph', children: parseInline(paragraph.join('\n')) });
    paragraph = [];
  };
  const endQuote = () => {
    if (quote.length) blocks.push({ type: 'quote', children: parseInline(quote.join('\n')) });
    quote = [];
  };
  for (let index = 0; index < lines.length; index++) {
    const line = lines[index];
    const fence = /^```([\w+-]{0,20})\s*$/u.exec(line);
    if (fence) {
      const close = lines.findIndex((candidate, at) => at > index && candidate.trim() === '```');
      if (close > index) {
        endParagraph();
        endQuote();
        blocks.push({ type: 'code', text: lines.slice(index + 1, close).join('\n'), language: fence[1] });
        index = close;
        continue;
      }
    }
    const quoted = /^>\s?(.*)$/u.exec(line);
    if (quoted) {
      endParagraph();
      quote.push(quoted[1]);
      continue;
    }
    endQuote();
    paragraph.push(line);
  }
  endParagraph();
  endQuote();
  if (source.length > MAX_PARSED) blocks.push({ type: 'paragraph', children: [{ type: 'text', text: source.slice(MAX_PARSED) }] });
  return blocks.length > 0 ? blocks : [{ type: 'paragraph', children: [{ type: 'text', text: source }] }];
}
