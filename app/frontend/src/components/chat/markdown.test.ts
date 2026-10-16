import { describe, expect, it } from 'vitest';
import { parseInline, parseMarkdown, safeHref, type Inline } from './markdown';

const text = (t: string): Inline => ({ type: 'text', text: t });

describe('inline', () => {
  it('bold, italic and code', () => {
    expect(parseInline('a **b** c *d* e `f`')).toEqual([
      text('a '), { type: 'bold', children: [text('b')] }, text(' c '), { type: 'italic', children: [text('d')] }, text(' e '), { type: 'code', text: 'f' },
    ]);
  });

  it('markers inside code stay literal, and unclosed markers stay text', () => {
    expect(parseInline('`**not bold**`')).toEqual([{ type: 'code', text: '**not bold**' }]);
    expect(parseInline('2 * 3 = 6 and **open')).toEqual([text('2 * 3 = 6 and **open')]);
    expect(parseInline('snake_case_name')).toEqual([text('snake_case_name')]);
  });

  it('bold may contain italic', () => {
    expect(parseInline('**a *b* c**')).toEqual([{ type: 'bold', children: [text('a '), { type: 'italic', children: [text('b')] }, text(' c')] }]);
  });

  it('links: autolinks without the full stop, brackets in addresses, and [text](url)', () => {
    expect(parseInline('see https://example.com/a_(b). ok')).toEqual([
      text('see '), { type: 'link', href: 'https://example.com/a_(b)', children: [text('https://example.com/a_(b)')] }, text('. ok'),
    ]);
    expect(parseInline('(https://example.com/x)')).toEqual([text('('), { type: 'link', href: 'https://example.com/x', children: [text('https://example.com/x')] }, text(')')]);
    expect(parseInline('[docs](https://example.com/docs)')).toEqual([{ type: 'link', href: 'https://example.com/docs', children: [text('docs')] }]);
  });

  it('only web addresses become links', () => {
    for (const bad of ['javascript:alert(1)', 'data:text/html,<b>x</b>', 'file:///C:/Windows', 'vbscript:x', '//evil.example', 'ftp://example.com']) {
      expect(safeHref(bad)).toBeNull();
      expect(parseInline(`[click](${bad})`).some((node) => node.type === 'link')).toBe(false);
    }
    expect(safeHref('HTTPS://Example.com/a b')).toBe('https://example.com/a%20b');
  });

  it('html stays text: nothing here ever becomes markup', () => {
    for (const evil of ['<script>alert(1)</script>', '<img src=x onerror=alert(1)>', '"><svg onload=alert(1)>', '**<b>x</b>**']) {
      const flat = JSON.stringify(parseInline(evil));
      expect(flat).not.toContain('"href"');
      expect(parseInline(evil).every((node) => ['text', 'bold'].includes(node.type))).toBe(true);
    }
  });

  it('nesting is limited and pathological input terminates', () => {
    const deep = '*'.repeat(500) + 'x' + '*'.repeat(500);
    expect(() => parseInline(deep)).not.toThrow();
    expect(() => parseInline('['.repeat(5000))).not.toThrow();
    expect(() => parseInline('http://' + 'a'.repeat(5000))).not.toThrow();
  });
});

describe('blocks', () => {
  it('paragraphs, quotes and fenced code', () => {
    expect(parseMarkdown('hello\n> quoted\n> more\n```js\nlet x = 1;\n**not bold**\n```\nbye')).toEqual([
      { type: 'paragraph', children: [text('hello')] },
      { type: 'quote', children: [text('quoted\nmore')] },
      { type: 'code', language: 'js', text: 'let x = 1;\n**not bold**' },
      { type: 'paragraph', children: [text('bye')] },
    ]);
  });

  it('an unclosed fence is plain text, and empty input still gives something to show', () => {
    expect(parseMarkdown('```\nno end')).toEqual([{ type: 'paragraph', children: [text('```\nno end')] }]);
    expect(parseMarkdown('')).toEqual([{ type: 'paragraph', children: [] }]);
  });

  it('very long text is cut for parsing but not lost', () => {
    const long = 'x'.repeat(20_000);
    const blocks = parseMarkdown(long);
    expect(JSON.stringify(blocks).length).toBeGreaterThanOrEqual(20_000);
  });
});
