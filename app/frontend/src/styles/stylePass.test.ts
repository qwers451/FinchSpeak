import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { expect, it } from 'vitest';
import { declarationsFor, readStyle } from './cssTest';

/** Components touched by the 1.7 style pass (docs/QA-1.7.0.md, "Стиль и вёрстка"). */
const RESTYLED_COMPONENTS = [
  'components/VoicePanel.tsx',
  'components/voice/SoundboardPanel.tsx',
  'components/voice/Dock.tsx',
  'components/voice/LiveCard.tsx',
  'components/chat/Composer.tsx',
  'components/tree/TreeView.tsx',
  'components/settings/SettingsModal.tsx',
  'components/settings/ThemeEditor.tsx',
];

/** Cards and panels of 1.6–1.7 that Finch Custom draws with cut corners. */
const CUT_CORNER_CARDS = [
  'music-panel',
  'soundboard-panel',
  'soundboard-tile',
  'health-tile',
  'voice-note',
  'flock-card',
  'overlay-card',
  'ctx-menu',
  'select-listbox',
];

/** Literal colours of the theme tokens. Their tints must be written with color-mix() of the token. */
const TOKEN_TRIPLES = ['255,194,14', '255,51,0', '0,180,255', '64,48,245', '0,184,188', '255,171,0', '14,19,32', '10,15,28'];

it('sets no colour or background inline in the components of the style pass', () => {
  for (const file of RESTYLED_COMPONENTS) {
    const source = readFileSync(join(process.cwd(), 'src', file), 'utf8');
    // A custom property such as '--swatch-color' is a dynamic value, not a literal colour.
    const inline = source.match(/style=\{\{[^}]*(?<![-\w])'?(?:color|background(?:-color)?)'?\s*:/g) ?? [];
    expect(inline, file).toEqual([]);
  }
});

it('lists every new card and panel in the Finch Custom cut-corner selector', () => {
  const list = readStyle('themes.css').match(/\[data-theme="custom"\] :is\(([^)]*)\)::after/)?.[1] ?? '';
  const selectors = list.split(',').map((s) => s.trim());
  for (const card of CUT_CORNER_CARDS) {
    expect(selectors, card).toContain(`.${card}`);
  }
});

it('uses the theme token for tints of the accent colours, not a literal rgba()', () => {
  for (const file of ['app.css', 'polish.css', 'nest.css', 'music.css', 'soundboard.css', 'voiceNote.css', 'spatial.css', 'user-theme.css']) {
    const css = readStyle(file);
    for (const triple of TOKEN_TRIPLES) {
      const literal = new RegExp(`rgba\\(\\s*${triple.replace(/,/g, '\\s*,\\s*')}\\s*,`);
      expect(css, `${file} ${triple}`).not.toMatch(literal);
    }
  }
});

it('measures the room column, not the whole room, for the header wrap beside the music panel', () => {
  const polish = readStyle('polish.css');
  expect(declarationsFor(polish, '.room-column').join(';')).toMatch(/container-type:\s*inline-size/);
  expect(declarationsFor(polish, '.voice-room').join(';')).not.toMatch(/container-type/);
});

it('keeps the header action and the music chip from squeezing into each other', () => {
  const polish = readStyle('polish.css');
  expect(declarationsFor(polish, '.mhead > .btn').join(';')).toMatch(/flex:\s*none/);
  expect(declarationsFor(polish, '.music-now-chip').join(';')).toMatch(/flex:\s*0\s+1/);
});

it('lifts the call cards above the dock so they do not cover it', () => {
  expect(declarationsFor(readStyle('app.css'), '.flock-stack').join(';')).toMatch(/bottom:\s*96px/);
});

it('lets the soundboard grid and the add row shrink in a narrow dialog', () => {
  const sound = readStyle('soundboard.css');
  expect(declarationsFor(sound, '.soundboard-add').join(';')).toMatch(/minmax\(0,\s*1fr\)/);
  expect(sound).toMatch(/minmax\(min\(120px,\s*100%\),\s*1fr\)/);
});

it('lets the health tiles and the voice note shrink with their container', () => {
  expect(readStyle('app.css')).toMatch(/minmax\(min\(176px,\s*100%\),\s*1fr\)/);
  expect(declarationsFor(readStyle('voiceNote.css'), '.voice-note').join(';')).toMatch(/max-width:\s*min\(340px,\s*100%\)/);
});

it('keeps the nest name clear of the actions that appear over it on hover', () => {
  expect(readStyle('nest.css')).toMatch(/\.nest:hover \.nest-top[^{]*\{\s*padding-right:\s*44px/);
});
