import { expect, it } from 'vitest';
import { declarationsFor, readStyle } from './cssTest';

it('keeps the top corners of the input straight under a reply bar', () => {
  const rules = declarationsFor(readStyle('app.css'), '.reply-bar + .composer-anchor > .composer').join(';');
  expect(rules).toMatch(/border-top-left-radius:\s*0/);
  expect(rules).toMatch(/border-top-right-radius:\s*0/);
});

it('anchors the composer popovers to the top edge of the input, whatever its height', () => {
  const app = readStyle('app.css');
  expect(declarationsFor(app, '.composer-anchor').join(';')).toMatch(/position:\s*relative/);
  for (const selector of ['.composer-picker', '.mention-menu']) {
    const rules = declarationsFor(app, selector).join(';');
    expect(rules, selector).toMatch(/bottom:\s*calc\(100%\s*\+\s*8px\)/);
    expect(rules, selector).not.toMatch(/bottom:\s*72px/);
  }
});

it('has no role-tag override left for the members panel: the role is named by the group heading', () => {
  expect(readStyle('themes.css')).not.toContain('.members .tag.admin');
});

const CASCADE = ['finch.css', 'app.css', 'custom-tokens.css', 'themes.css', 'motion.css', 'polish.css'];
const fontSize = (selector: string) => {
  const sizes = CASCADE.flatMap((file) => declarationsFor(readStyle(file), selector)).flatMap((rule) => Array.from(rule.matchAll(/font-size:\s*(\d+)px/g), (m) => Number(m[1])));
  return sizes[sizes.length - 1];
};

it('draws day separators at 12px in Claude Dark and 11px in Finch Custom', () => {
  expect(fontSize('.daysep')).toBe(12);
  expect(fontSize('[data-theme="custom"] .daysep')).toBe(11);
});

it('keeps the Finch Custom separator uppercase with tracking', () => {
  const rules = declarationsFor(readStyle('themes.css'), '[data-theme="custom"] .daysep').join(';');
  expect(rules).toMatch(/text-transform:\s*uppercase/);
  expect(rules).toMatch(/letter-spacing:\s*\.08em/);
});

it('leaves the base separator size in finch.css alone: the theme rules own it', () => {
  expect(fontSize('.daysep')).not.toBe(11);
  expect(declarationsFor(readStyle('finch.css'), '.daysep').join(';')).toMatch(/font:800 11px/);
});
