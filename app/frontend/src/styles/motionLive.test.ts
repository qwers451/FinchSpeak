import { expect, it } from 'vitest';
import { declarationsFor, readStyle } from './cssTest';

const css = readStyle('motion.css');
const live = css.slice(css.indexOf('/* ---- Living motion'));
const stripComments = (text: string) => text.replace(/\/\*[\s\S]*?\*\//g, '');

/** Selectors of every rule (keyframe steps included) whose declarations contain `token`. */
function selectorsUsing(text: string, token: string): string[] {
  const stripped = stripComments(text);
  const out: string[] = [];
  const re = /([^{}]+)\{([^{}]*)\}/g;
  let match: RegExpExecArray | null;
  while ((match = re.exec(stripped))) if (match[2].includes(token)) out.push(match[1].trim());
  return out;
}

/** Property names set in each @keyframes block of `text`. */
function keyframeProperties(text: string): Map<string, string[]> {
  const stripped = stripComments(text);
  const found = new Map<string, string[]>();
  const re = /@keyframes\s+([\w-]+)\s*\{((?:[^{}]*\{[^{}]*\})*)\s*\}/g;
  let match: RegExpExecArray | null;
  while ((match = re.exec(stripped))) {
    const props = [...match[2].matchAll(/\{([^{}]*)\}/g)].flatMap((step) => [...step[1].matchAll(/([a-z-]+)\s*:/g)].map((prop) => prop[1]));
    found.set(match[1], props);
  }
  return found;
}

it('defines a keyframe block for every animation the live motion uses', () => {
  const keyframes = keyframeProperties(css);
  const names = [...live.matchAll(/animation(?:-name)?:\s*([\w-]+)/g)].map((match) => match[1]);
  const used = names.filter((name) => !['none', 'calc', 'var'].includes(name));
  expect(used.length).toBeGreaterThan(10);
  for (const name of used) expect(keyframes.has(name), name).toBe(true);
});

it('animates only transform, opacity, clip-path and the stroke dash offset', () => {
  const allowed = new Set(['transform', 'opacity', 'clip-path', 'stroke-dashoffset']);
  for (const [name, props] of keyframeProperties(css)) {
    if (!/^(msg-in|msg-in-own|tile-in|tile-out|icon-turn|card-in|said-float|bird-|theme-reveal|nest-sway|bird-hop|spark-draw|flock-drain)/.test(name)) continue;
    for (const prop of props) expect(allowed.has(prop), `${name} animates ${prop}`).toBe(true);
  }
});

it('never transitions a layout property in the live motion', () => {
  const layout = new Set(['width', 'height', 'top', 'left', 'right', 'bottom', 'margin', 'padding', 'gap', 'flex', 'max-width', 'min-height']);
  const declared = [...stripComments(live).matchAll(/transition(?:-property)?:\s*([^;}]+)/g)]
    .flatMap((match) => match[1].split(',').map((part) => part.trim().split(/\s+/)[0]));
  expect(declared.length).toBeGreaterThan(3);
  expect(declared.filter((prop) => layout.has(prop))).toEqual([]);
});

it('keeps the bird on its tempo: sway 4 s, blink every 5 s for about 120 ms, waves 1.2 s with a 200 ms offset', () => {
  expect(css).toMatch(/html:not\(\[data-reduce-motion="true"\]\) \.finch-bird \{ animation:bird-sway 4s ease-in-out infinite;/);
  expect(css).toMatch(/\.finch-bird \.bird-eye \{[^}]*animation:bird-blink 5s/);
  expect(css).toMatch(/\.finch-bird \.bird-wave \{ animation:bird-wave 1\.2s ease-in-out infinite;/);
  expect(css).toMatch(/\.finch-bird \.bird-wave \+ \.bird-wave \{ animation-delay:200ms;/);
  expect(css).toMatch(/@keyframes bird-blink \{[\s\S]*?97\.2% \{ transform:scaleY\(\.1\)/);
  expect(css).toMatch(/@keyframes bird-sway \{ 0%, 100% \{ transform:rotate\(-2deg\)[\s\S]*?50% \{ transform:rotate\(2deg\)/);
});

it('uses the spring curve only on small pop elements and card entries', () => {
  const users = selectorsUsing(css, 'var(--ease-spring)').filter((selector) => selector !== ':root');
  expect(users.sort()).toEqual(['.dock .ctl', '.flock-card.flock-enter', '.nest-bird.nest-bird-enter', '.toast'].sort());
});

it('reveals the theme as a circle from the click point', () => {
  expect(css).toMatch(/::view-transition-new\(root\) \{ animation:theme-reveal var\(--motion-reveal\)/);
  expect(declarationsFor(css, '::view-transition-new(root)').join()).toContain('theme-reveal');
  expect(css).toMatch(/@keyframes theme-reveal \{ from \{ clip-path:circle\(0px at var\(--reveal-x/);
});

it('switches every live animation off with the app setting and with the system preference', () => {
  expect(css).toMatch(/@media \(prefers-reduced-motion:reduce\) \{\s*\*,\*::before,\*::after \{ animation:none!important; transition:none!important;/);
  expect(css).toMatch(/\[data-reduce-motion="true"\] \*,\[data-reduce-motion="true"\] \*::before,\[data-reduce-motion="true"\] \*::after \{ animation:none!important; transition:none!important;/);
  expect(css).toMatch(/html\[data-reduce-motion="true"\] ::view-transition-new\(root\) \{ animation:none; \}/);
});

it('keeps the flock strip, the ripple and the name bubble in their own rules', () => {
  expect(declarationsFor(css, '.flock-timer::after').join()).toContain('flock-drain 30s linear forwards');
  expect(declarationsFor(css, '.ripple-dot').join()).toContain('pointer-events:none');
  expect(declarationsFor(css, '.vc .sb-said').join()).toContain('said-float 1.2s');
});

const spatial = readStyle('spatial.css');

it('moves a spatial seat with transform only, never with a transition on left or top', () => {
  const seat = declarationsFor(spatial, '.seat').join(';');
  expect(seat).toMatch(/transition:transform 260ms var\(--ease-out\)/);
  expect(seat).not.toMatch(/transition:[^;]*\b(left|top)\b/);
  expect(declarationsFor(spatial, '.seat.moving').join(';')).not.toMatch(/transition:[^;]*transform/);
});

it('measures the stage as a size container, so a seat moves sideways in cqw inside a transform', () => {
  expect(declarationsFor(spatial, '.stage-area').join(';')).toMatch(/container-type:inline-size/);
});

it('switches the spatial seat movement off with the app setting and with the system preference', () => {
  expect(spatial).toMatch(/@media \(prefers-reduced-motion:reduce\) \{ \.seat \{ transition:none; \} \}/);
  expect(spatial).toMatch(/\[data-reduce-motion="true"\] \.seat \{ transition:none; \}/);
});