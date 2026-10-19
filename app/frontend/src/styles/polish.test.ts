import { expect, it } from 'vitest';
import { declarationsFor, readStyle } from './cssTest';

it('makes the members panel scroll inside its column', () => {
  const rules = declarationsFor(readStyle('polish.css'), '.members').join(';');
  expect(rules).toMatch(/min-height:\s*0/);
  expect(rules).toMatch(/overflow-y:\s*auto/);
  expect(rules).toMatch(/scrollbar-gutter:\s*stable/);
  expect(rules).toMatch(/overscroll-behavior:\s*contain/);
});

it('does not reset members scrolling in the base or theme layers', () => {
  const lower = [
    ...declarationsFor(readStyle('finch.css'), '.members'),
    ...declarationsFor(readStyle('themes.css'), '.members'),
    ...declarationsFor(readStyle('themes.css'), '[data-theme="custom"] .members'),
  ].join(';');
  expect(lower).not.toMatch(/overflow/);
});
