import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { expect, it } from 'vitest';
import { readStyle } from './cssTest';

const source = (rel: string) => readFileSync(join(process.cwd(), 'src', rel), 'utf8');
const panelSource = source('components/music/MusicPanel.tsx');
const voiceSource = source('components/VoicePanel.tsx');
const musicCss = readStyle('music.css');
/** Rules a music class may come from: music.css, or the shared base (finch.css tabs and fields, app.css and polish.css helpers). */
const allCss = [musicCss, readStyle('finch.css'), readStyle('app.css'), readStyle('polish.css')].join('\n').replace(/\/\*[\s\S]*?\*\//g, '');

/** True when some rule in `css` contains the selector `.a` or the compound `.a.b`. */
const hasRule = (css: string, path: string[]) => new RegExp(path.map((name) => `\\.${name}`).join('') + '(?![\\w-])').test(css);

/** Class names and class modifiers that a TSX file can produce, read from its source text. */
function classUsage(src: string) {
  const classes = new Set<string>();
  const compounds = new Set<string>();
  const statusNames = (src.match(/status:\s*([^;]+);/)?.[1].match(/'([a-z]+)'/g) ?? []).map((quoted) => quoted.slice(1, -1));

  for (const match of src.matchAll(/class(?:Name)?=\{?\s*"([^"]*)"/g)) match[1].split(/\s+/).filter(Boolean).forEach((name) => classes.add(name));
  for (const match of src.matchAll(/class(?:Name)?=\{\s*'([^']*)'\s*\}/g)) match[1].split(/\s+/).filter(Boolean).forEach((name) => classes.add(name));

  for (const match of src.matchAll(/`([^`]*)`/g)) {
    const raw = match[1];
    const head = raw.split('${')[0].trim();
    if (!/^[a-z][a-z0-9-]*(\s+[a-z][a-z0-9-]*)*$/.test(head)) continue;
    head.split(/\s+/).forEach((name) => classes.add(name));
    const base = head.split(/\s+/).pop() ?? '';
    for (const expr of raw.match(/\$\{[^}]*\}/g) ?? []) {
      for (const quoted of expr.matchAll(/'\s*([a-z][a-z0-9-]*)\s*'/g)) compounds.add(`${base}.${quoted[1]}`);
      if (/\.status\b/.test(expr)) statusNames.forEach((state) => compounds.add(`${base}.${state}`));
    }
  }
  return { classes: [...classes], compounds: [...compounds] };
}

it('every class and modifier MusicPanel.tsx produces has a CSS rule', () => {
  const usage = classUsage(panelSource);
  expect(usage.classes).toContain('music-panel');
  expect(usage.compounds).toEqual(expect.arrayContaining(['tab.on', 'music-upload.uploading', 'music-upload.done', 'music-upload.error']));

  const missing = [
    ...usage.classes.filter((name) => !hasRule(allCss, [name])).map((name) => `.${name}`),
    ...usage.compounds.filter((pair) => !hasRule(allCss, pair.split('.'))).map((pair) => `.${pair}`),
  ];
  expect(missing).toEqual([]);
});

it('music.css uses theme variables only, no hex colours', () => {
  const stripped = musicCss.replace(/\/\*[\s\S]*?\*\//g, '');
  expect(stripped.match(/#[0-9a-fA-F]{3,8}\b/g) ?? []).toEqual([]);
});

it('the now-playing card and the clock use theme tokens and tabular digits', () => {
  expect(musicCss).toMatch(/\.music-now\s*\{[^}]*background:\s*var\(--bg-card\)/);
  expect(musicCss).toMatch(/\.music-clock\s*\{[^}]*font-variant-numeric:\s*tabular-nums/);
});

it('the music chip in the room header uses the same clock class', () => {
  expect(voiceSource).toContain('class="t3 fs12 music-clock"');
  expect(voiceSource).not.toContain('tabular');
});

it('the panel lives in an app dialog: full width, transparent surface, the custom theme drops the panel edge', () => {
  expect(musicCss).toMatch(/.music-panel {[^}]*width: 100%[^}]*background: transparent/);
  expect(readStyle('themes.css')).toContain('[data-theme="custom"] .modal .music-panel::before, [data-theme="custom"] .modal .music-panel::after { display:none; }');
});

it('the tab labels truncate with an ellipsis instead of wrapping at 280 px', () => {
  expect(musicCss).toMatch(/\.music-add \.tab \{[^}]*white-space: nowrap/);
  expect(panelSource).toMatch(/<span class="trunc">\{t\('Library'\)\}<\/span>/);
  expect(panelSource).toMatch(/<span class="trunc">\{t\('From my computer'\)\}<\/span>/);
});
