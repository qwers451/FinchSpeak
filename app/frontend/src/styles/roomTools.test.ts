import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { expect, it } from 'vitest';
import { declarationsFor, readStyle } from './cssTest';

const source = (rel: string) => readFileSync(join(process.cwd(), 'src', rel), 'utf8');
const strip = (css: string) => css.replace(/\/\*[\s\S]*?\*\//g, '');
const room = strip(readStyle('roomTools.css'));
const music = strip(readStyle('music.css'));

/** Body of the first @media block whose query contains `query`. */
function media(css: string, query: string): string {
  const start = css.indexOf(`@media ${query}`);
  if (start < 0) return '';
  const open = css.indexOf('{', start);
  let depth = 0;
  for (let i = open; i < css.length; i += 1) {
    if (css[i] === '{') depth += 1;
    if (css[i] === '}' && --depth === 0) return css.slice(open + 1, i);
  }
  return '';
}

it('is loaded by the app entry point', () => {
  expect(source('index.tsx')).toContain('./styles/roomTools.css');
});

it('hides the button labels at the narrow panel width (232 px) and in compact mode, leaving icons', () => {
  expect(media(room, '(max-width:1150px)')).toMatch(/\.room-tools\s+\.rt-label\s*\{[^}]*display:\s*none/);
  expect(declarationsFor(room, '.room-tools.compact .rt-label').join(';')).toMatch(/display:\s*none/);
});

it('keeps the channel list scrollable above the block', () => {
  expect(declarationsFor(strip(readStyle('finch.css')), '.nav').join(';')).toMatch(/min-height:\s*0/);
  expect(declarationsFor(strip(readStyle('themes.css')), '.nav').join(';')).toMatch(/overflow-y:\s*auto/);
  expect(declarationsFor(room, '.room-tools').join(';')).toMatch(/flex:\s*none/);
});

it('drops the clock of the music row on short windows', () => {
  expect(media(room, '(max-height:700px)')).toMatch(/\.room-now\s+\.music-clock\s*\{[^}]*display:\s*none/);
});

it('truncates the track title and uses theme variables only', () => {
  expect(declarationsFor(room, '.room-now .trunc').join(';')).toMatch(/min-width:\s*0/);
  expect(room.match(/#[0-9a-fA-F]{3,8}\b/g) ?? []).toEqual([]);
});

it('the side column of the music panel is gone', () => {
  expect(music).not.toContain('music-open');
  expect(music).not.toMatch(/width:\s*340px/);
  expect(source('components/VoicePanel.tsx')).not.toMatch(/music-open|sidePanel|<MusicPanel|<SoundboardPanel/);
});

it('the music panel keeps its rows inside the dialog at 340 and 480 px: shrinking text, wrapping controls', () => {
  const row = declarationsFor(music, '.music-row').join(';');
  expect(row).toMatch(/min-width:\s*0/);
  expect(row).toMatch(/flex-wrap:\s*wrap/);
  const text = declarationsFor(music, '.music-row .col').join(';');
  expect(text).toMatch(/min-width:\s*0/);
  expect(text).toMatch(/flex:\s*1 1/);
  expect(declarationsFor(music, '.music-controls').join(';')).toMatch(/flex-wrap:\s*wrap/);
  expect(declarationsFor(music, '.music-body').join(';')).toMatch(/min-width:\s*0/);
  expect(declarationsFor(music, '.music-panel').join(';')).toMatch(/width:\s*100%/);
});

it('the music panel titles carry a title tooltip and the search uses the shared Field', () => {
  const panel = source('components/music/MusicPanel.tsx');
  expect(panel).toMatch(/class="b trunc" title=\{now\(\)\.entry\.track\.title\}/);
  expect(panel).toMatch(/<span class="trunc" title=\{item\.(track\.)?title\}>/);
  expect(panel).toContain('<Field');
  expect(panel).not.toMatch(/class="field music-search"/);
});
