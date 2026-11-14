import { expect, it } from 'vitest';
import { declarationsFor, readStyle } from './cssTest';

// The colour swatches of "My colors" overhang their box: the hidden colour input is inset by -8px and the
// focus ring is 2px wide with a 3px offset. `.settings-page` is a scroll box (overflow:auto) and clips anything
// left of its padding edge, so it needs at least 8px of left padding for the swatches of the first column.
it('leaves 8px of left padding in the settings page for the swatch overhang', () => {
  const decls = declarationsFor(readStyle('themes.css'), '.settings-page').join(';');
  const padding = /padding\s*:\s*[\w.]+\s+[\w.]+\s+[\w.]+\s+([\w.]+)/.exec(decls);
  expect(padding, 'padding of .settings-page').not.toBeNull();
  expect(Number.parseFloat(padding?.[1] ?? '0')).toBeGreaterThanOrEqual(8);
});
