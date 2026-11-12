import { expect, it } from 'vitest';
import { declarationsFor, readStyle } from './cssTest';

const css = readStyle('motion.css');
const selector = (style: string, target: string) => `:where(:not([data-reduce-motion="true"]))[data-speaking-style="${style}"] .av.speaking ${target}`;

it('defines the effect for every speaking style', () => {
  expect(declarationsFor(css, selector('pulse', '.av-speaking-ring')).join()).toContain('var(--lvl');
  expect(declarationsFor(css, '[data-speaking-style="ring"] .av.speaking .av-speaking-ring').join()).toContain('scale(1)');
  expect(declarationsFor(css, selector('waves', '.av-speaking-ring::before')).join()).toContain('1.4s');
  expect(declarationsFor(css, selector('bars', '.av-bars')).join()).toContain('display:flex');
  expect(declarationsFor(css, selector('glow', '.av-speaking-glow')).join()).toContain('var(--lvl');
});

it('leaves the static ring and disables its transition for either reduced-motion setting', () => {
  expect(css).toMatch(/@media\s*\(prefers-reduced-motion:\s*no-preference\)\s*\{\s*:where\(:not\(\[data-reduce-motion="true"\]\)\)\[data-speaking-style="pulse"\]/);
  expect(css).toMatch(/@media\s*\(prefers-reduced-motion:\s*reduce\)\s*\{[^}]+transition:none/);
  expect(declarationsFor(css, '[data-reduce-motion="true"] *').join()).toContain('transition:none');
  expect(declarationsFor(css, '.av.speaking .av-speaking-ring').join()).toContain('scale(1)');
});
