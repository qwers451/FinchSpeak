import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { expect, it } from 'vitest';
import { declarationsFor, readStyle } from './cssTest';

const panelSource = readFileSync(join(process.cwd(), 'src', 'components', 'music', 'MusicPanel.tsx'), 'utf8');
const musicCss = readStyle('music.css');

/**
 * Classes that MusicPanel uses from shared UI components or global CSS (finch.css / app.css), not from music.css.
 * Keep this list short: a music-specific class must get its own rule in music.css.
 * `tabs`, `tab`, `on` come from the shared tabs (SharePickerModal); `col`, `row`, `g8`, `grow`, `trunc`, `t2`, `t3`,
 * `fs12`, `fs13`, `b`, `field` are the global layout and text helpers.
 */
const SHARED_CLASSES = new Set(['tabs', 'tab', 'on', 'col', 'row', 'g8', 'grow', 'trunc', 't2', 't3', 'fs12', 'fs13', 'b', 'field']);

/** Literal class names used by MusicPanel.tsx: class="…", class={`…`} and the class-name template in tabClass. */
function classNamesInPanel(source: string): string[] {
  const names = new Set<string>();
  const literal = /class(?:Name)?=\{?\s*(["'`])([\s\S]*?)\1/g;
  const templateBase = /`([^`]*)`/g;
  const collect = (raw: string) => {
    raw.replace(/\$\{[^}]*\}/g, ' ').split(/\s+/).filter(Boolean).forEach((name) => names.add(name));
  };
  let match: RegExpExecArray | null;
  while ((match = literal.exec(source))) collect(match[2]);
  while ((match = templateBase.exec(source))) if (/^music-/.test(match[1])) collect(match[1]);
  return [...names].filter((name) => /^[a-z][a-z0-9-]*$/.test(name));
}

it('every class MusicPanel.tsx uses has a rule in music.css or is a shared class', () => {
  const used = classNamesInPanel(panelSource);
  expect(used).toContain('music-panel');
  const missing = used.filter((name) => !SHARED_CLASSES.has(name) && !new RegExp(`\\.${name}\\b`).test(musicCss));
  expect(missing).toEqual([]);
});

it('music.css uses theme variables only, no hex colours', () => {
  const stripped = musicCss.replace(/\/\*[\s\S]*?\*\//g, '');
  expect(stripped.match(/#[0-9a-fA-F]{3,8}\b/g) ?? []).toEqual([]);
});

it('the now-playing card and the clock use theme tokens and tabular digits', () => {
  expect(declarationsFor(musicCss, '.music-now').join(';')).toMatch(/var\(--bg-card\)/);
  expect(declarationsFor(musicCss, '.music-clock').join(';')).toMatch(/tabular-nums/);
});
