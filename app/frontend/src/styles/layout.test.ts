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
