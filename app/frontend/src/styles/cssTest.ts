import { readFileSync } from 'node:fs';
import { join } from 'node:path';

/** Test helpers for the style sources. Vitest replaces imported CSS with an empty string, so files are read from disk. */
export const readStyle = (name: string) => readFileSync(join(process.cwd(), 'src', 'styles', name), 'utf8');

/** Declarations of every top-level rule (outside @media) whose selector list contains `selector`. */
export function declarationsFor(css: string, selector: string): string[] {
  const stripped = css.replace(/\/\*[\s\S]*?\*\//g, '');
  const out: string[] = [];
  const re = /([^{}@]+)\{([^{}]*)\}/g;
  let match: RegExpExecArray | null;
  while ((match = re.exec(stripped))) {
    if (match[1].split(',').map((s) => s.trim()).includes(selector)) out.push(match[2]);
  }
  return out;
}
