import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { expect, it } from 'vitest';
import { declarationsFor, readStyle } from './cssTest';

// Fixes from the 1.7.1 visual audit (docs/QA-1.7.1.md, "Визуальный аудит"): every place that overflowed or overlapped.

const source = (rel: string) => readFileSync(join(process.cwd(), 'src', rel), 'utf8');
const strip = (css: string) => css.replace(/\/\*[\s\S]*?\*\//g, '');
const all = ['finch.css', 'app.css', 'themes.css', 'motion.css', 'polish.css', 'music.css', 'roomTools.css', 'nest.css'].map((name) => strip(readStyle(name))).join('\n');
const rule = (selector: string) => declarationsFor(all, selector).join(';');

it('a tag never wraps its label to a second line (the LIVE tag in the Russian channel row did)', () => {
  expect(rule('.tag')).toMatch(/white-space:\s*nowrap/);
  expect(rule('.tag')).toMatch(/flex:\s*none|flex-shrink:\s*0/);
  // a long tag is cut with an ellipsis, not allowed to push its row
  expect(rule('.tag')).toMatch(/max-width:\s*100%/);
  expect(rule('.tag')).toMatch(/text-overflow:\s*ellipsis/);
});

it('the admin table header wraps a long label instead of spilling out of its cell', () => {
  expect(rule('.members-table th')).toMatch(/overflow-wrap:\s*anywhere/);
});

it('the music progress slider shows no value bubble over the artist line (the clock already says the position)', () => {
  expect(rule('.music-progress .range-value')).toMatch(/display:\s*none/);
});

it('the Finch mode switch has its own row under the brand: beside the name its Russian label ran 23 px into the next column', () => {
  const oldRule = /\.brand\s+\.brand-finch\s*\{/;
  expect(rule('.finch-switch')).toMatch(/display:\s*flex/);
  expect(rule('.finch-switch .brand-finch')).toMatch(/min-width:\s*0/);
  expect(all).not.toMatch(oldRule);
  expect('.brand .brand-finch { margin-left:auto; }').toMatch(oldRule); // the guard does catch the old rule
  expect(source('components/Sidebar.tsx')).toMatch(/<div class="finch-switch">/);
});

it('the perch of Finch mode wraps instead of overlapping, and its voice status can shrink', () => {
  expect(rule('.tree-perch')).toMatch(/flex-wrap:\s*wrap/);
  expect(rule('.tree-perch .tree-voice')).toMatch(/min-width:\s*0/);
});

it('the compact room tools (Finch mode perch) truncate the track title and keep their buttons inside', () => {
  expect(rule('.room-tools.compact .room-now')).toMatch(/flex-direction:\s*row/);
  expect(rule('.room-tools.compact .room-now')).toMatch(/min-width:\s*0/);
  expect(rule('.room-tools.compact .room-now')).toMatch(/max-width:\s*\d+px/);
  expect(rule('.room-tools.compact .room-now-actions .btn')).toMatch(/flex:\s*none/);
});

it('truncated names carry a title tooltip with the full text', () => {
  expect(source('components/MembersPanel.tsx')).toMatch(/<span class="trunc" title=\{props\.user\.display_name \|\| props\.user\.username\}>/);
  expect(source('components/Sidebar.tsx')).toMatch(/<span class="grow trunc" title=\{participant\.username\}>/);
  expect(source('components/VoicePanel.tsx')).toMatch(/<h3 class="room-title" title=\{/);
  expect(source('components/ChatArea.tsx')).toMatch(/channel-topic" title=\{activeChannel\(\)\?\.topic/);
  expect(source('components/chat/MessageItem.tsx')).toMatch(/<span class="trunc" title=\{quote\(\)\.snippet\}>/);
  expect(source('components/chat/MessageItem.tsx')).toMatch(/<span class="t3 fs12 trunc" title=\{msg\(\)\.failure\}>/);
});

it('the Finch mode perch shows the full nest name as a title when it is truncated', () => {
  expect(source('components/tree/TreeView.tsx')).toContain(`<span class="t2 fs12 trunc" title={t('In the nest: {channel}'`);
});

it('the music rows and upload lines carry the full text as a title', () => {
  const panel = source('components/music/MusicPanel.tsx');
  expect(panel).toMatch(/<span class="t3 fs12 trunc" title=\{/);
  expect(panel.match(/<span class="t3 fs12 trunc" title=\{/g)!.length).toBeGreaterThanOrEqual(2);
  expect(panel).toMatch(/<span class="trunc" title=\{uploadText\(upload\)\}>/);
});

it('the fixture screens of the audit are reachable only on the dev server (import.meta.env.DEV guards the import)', () => {
  expect(source('index.tsx')).toMatch(/import\.meta\.env\.DEV && new URLSearchParams\(window\.location\.search\)\.has\('qa'\)[\s\S]*import\('\.\/qa\/qa'\)/);
  expect(source('App.tsx')).not.toContain('qa/');
});
