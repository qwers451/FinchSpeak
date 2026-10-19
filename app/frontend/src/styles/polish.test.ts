import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { expect, it } from 'vitest';

// Vitest replaces imported CSS with an empty string, so the style sources are read from disk.
const read = (name: string) => readFileSync(join(process.cwd(), 'src', 'styles', name), 'utf8');

/** Declarations of every top-level rule (outside @media) whose selector list contains `selector`. */
function declarationsFor(css: string, selector: string): string[] {
  const stripped = css.replace(/\/\*[\s\S]*?\*\//g, '');
  const out: string[] = [];
  const re = /([^{}@]+)\{([^{}]*)\}/g;
  let match: RegExpExecArray | null;
  while ((match = re.exec(stripped))) {
    if (match[1].split(',').map((s) => s.trim()).includes(selector)) out.push(match[2]);
  }
  return out;
}

it('makes the members panel scroll inside its column', () => {
  const rules = declarationsFor(read('polish.css'), '.members').join(';');
  expect(rules).toMatch(/min-height:\s*0/);
  expect(rules).toMatch(/overflow-y:\s*auto/);
  expect(rules).toMatch(/scrollbar-gutter:\s*stable/);
  expect(rules).toMatch(/overscroll-behavior:\s*contain/);
});

it('does not reset members scrolling in the base or theme layers', () => {
  const lower = [
    ...declarationsFor(read('finch.css'), '.members'),
    ...declarationsFor(read('themes.css'), '.members'),
    ...declarationsFor(read('themes.css'), '[data-theme="custom"] .members'),
  ].join(';');
  expect(lower).not.toMatch(/overflow/);
});
