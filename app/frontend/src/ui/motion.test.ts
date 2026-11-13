import { createRoot, createSignal } from 'solid-js';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { readStyle } from '../styles/cssTest';
import { animateLayout, canRevealTheme, createPresence, EASE, MOTION, popIn, prefersReducedMotion, revealTheme, staggerDelay } from './motion';

const motionCss = readStyle('motion.css');
const themesCss = readStyle('themes.css');
const cssVar = (name: string) => new RegExp(`--${name}:\\s*([^;}]+)`).exec(motionCss)?.[1].trim();

type Animatable = HTMLElement & { animate: ReturnType<typeof vi.fn> };
const withAnimate = (element: HTMLElement): Animatable => {
  const target = element as Animatable;
  target.animate = vi.fn(() => ({ onfinish: null }) as never);
  return target;
};
const setRect = (element: HTMLElement, left: number, top: number) => {
  element.getBoundingClientRect = () => ({ left, top, right: left + 100, bottom: top + 40, width: 100, height: 40, x: left, y: top, toJSON() {} }) as DOMRect;
};

beforeEach(() => {
  document.documentElement.removeAttribute('data-reduce-motion');
  document.body.innerHTML = '';
});

afterEach(() => {
  Reflect.deleteProperty(document, 'startViewTransition');
  document.documentElement.removeAttribute('data-reduce-motion');
  vi.useRealTimers();
});

it('keeps the JS timing tokens equal to the CSS custom properties', () => {
  expect(cssVar('motion-small')).toBe(`${MOTION.small}ms`);
  expect(cssVar('motion-panel')).toBe(`${MOTION.panel}ms`);
  expect(cssVar('motion-enter')).toBe(`${MOTION.enter}ms`);
  expect(cssVar('motion-exit')).toBe(`${MOTION.exit}ms`);
  expect(cssVar('motion-base')).toBe(`${MOTION.base}ms`);
  expect(cssVar('motion-pop')).toBe(`${MOTION.pop}ms`);
  expect(cssVar('motion-reveal')).toBe(`${MOTION.reveal}ms`);
  expect(cssVar('motion-stagger')).toBe(`${MOTION.stagger}ms`);
  expect(cssVar('ease-spring')).toBe(EASE.spring);
});

it('takes the ease-out curve from the same token as the themes (Claude Dark)', () => {
  expect(themesCss).toContain(`--ease-out:${EASE.out}`);
});

it('keeps every token duration inside the 120-280 ms range', () => {
  for (const value of Object.values(MOTION)) {
    if (value === MOTION.stagger || value === MOTION.maxStagger) continue;
    expect(value).toBeGreaterThanOrEqual(120);
    expect(value).toBeLessThanOrEqual(280);
  }
});

it('staggers 30 ms per item and never more than eight items', () => {
  expect(staggerDelay(0)).toBe(0);
  expect(staggerDelay(1)).toBe(30);
  expect(staggerDelay(-1)).toBe(0);
  expect(staggerDelay(7)).toBe(210);
  expect(staggerDelay(8)).toBe(210);
  expect(staggerDelay(50)).toBe(210);
});

it('reads reduced motion from the app setting and from the system', () => {
  expect(prefersReducedMotion()).toBe(false);
  document.documentElement.dataset.reduceMotion = 'true';
  expect(prefersReducedMotion()).toBe(true);
});

it('animateLayout glides moved tiles with transform only, and does nothing under reduced motion', () => {
  const container = document.createElement('div');
  const a = withAnimate(document.createElement('div'));
  const b = withAnimate(document.createElement('div'));
  a.dataset.flip = 'a';
  b.dataset.flip = 'b';
  container.append(a, b);
  document.body.append(container);
  setRect(a, 0, 0);
  setRect(b, 100, 0);
  expect(animateLayout(container)).toBe(0);

  // b and a swap places: both moved.
  setRect(a, 100, 0);
  setRect(b, 0, 0);
  expect(animateLayout(container)).toBe(2);
  expect(a.animate).toHaveBeenCalledWith(
    [{ transform: 'translate(-100px, 0px)' }, { transform: 'translate(0, 0)' }],
    expect.objectContaining({ duration: MOTION.base, easing: EASE.out }),
  );
  const keyframes = a.animate.mock.calls[0][0] as Array<Record<string, string>>;
  for (const frame of keyframes) expect(Object.keys(frame)).toEqual(['transform']);

  // A tile that did not move is left alone.
  a.animate.mockClear();
  setRect(a, 100, 0);
  setRect(b, 0, 0);
  expect(animateLayout(container)).toBe(0);
  expect(a.animate).not.toHaveBeenCalled();
});

it('animateLayout animates nothing under reduced motion and leaves the final positions in place', () => {
  document.documentElement.dataset.reduceMotion = 'true';
  const container = document.createElement('div');
  const a = withAnimate(document.createElement('div'));
  a.dataset.flip = 'a';
  container.append(a);
  setRect(a, 0, 0);
  animateLayout(container);
  setRect(a, 80, 0);
  expect(animateLayout(container)).toBe(0);
  expect(a.animate).not.toHaveBeenCalled();
});

it('popIn does nothing under reduced motion and pops otherwise', () => {
  const element = withAnimate(document.createElement('span'));
  document.documentElement.dataset.reduceMotion = 'true';
  expect(popIn(element)).toBeUndefined();
  expect(element.animate).not.toHaveBeenCalled();
  document.documentElement.dataset.reduceMotion = 'false';
  popIn(element);
  expect(element.animate).toHaveBeenCalledWith(expect.any(Array), expect.objectContaining({ duration: MOTION.pop }));
});

it('a theme switch runs at once when the View Transitions API is missing', () => {
  const paint = vi.fn();
  expect(canRevealTheme()).toBe(false);
  expect(revealTheme(paint)).toBe(false);
  expect(paint).toHaveBeenCalledTimes(1);
});

it('a theme switch is deferred into a view transition when the API exists and motion is allowed', () => {
  const start = vi.fn();
  (document as Document & { startViewTransition?: unknown }).startViewTransition = start;
  const paint = vi.fn();
  expect(canRevealTheme()).toBe(true);
  expect(canRevealTheme(true)).toBe(false);
  expect(revealTheme(paint)).toBe(true);
  expect(start).toHaveBeenCalledWith(paint);
  expect(paint).not.toHaveBeenCalled();
  expect(revealTheme(paint, true)).toBe(false);
  expect(paint).toHaveBeenCalledTimes(1);
});

it('the theme reveal starts from the last click point, kept on the html element', () => {
  document.body.innerHTML = '<button id="pick">Pick</button>';
  document.getElementById('pick')!.dispatchEvent(new MouseEvent('click', { clientX: 40, clientY: 52, detail: 1, bubbles: true }));
  expect(document.documentElement.style.getPropertyValue('--reveal-x')).toBe('40px');
  expect(document.documentElement.style.getPropertyValue('--reveal-y')).toBe('52px');
});

it('createPresence keeps a removed item for one exit and gives every key one stable entry', () => {
  vi.useFakeTimers();
  const [people, setPeople] = createSignal([{ id: 'a', name: 'Ann' }, { id: 'b', name: 'Bob' }]);
  let list!: ReturnType<typeof createPresence<{ id: string; name: string }>>;
  const dispose = createRoot((done) => {
    list = createPresence(people, (person) => person.id, 140);
    return done;
  });
  // The first value is not "entering".
  expect(list().map((entry) => entry.entering)).toEqual([false, false]);
  const annEntry = list()[0];

  // Ann stays, with a new object: the entry object is the same, so a list keyed by it keeps the node.
  setPeople([{ id: 'a', name: 'Ann renamed' }, { id: 'b', name: 'Bob' }]);
  expect(list()[0]).toBe(annEntry);
  expect(annEntry.item().name).toBe('Ann renamed');

  // Bob leaves: he is still listed, marked leaving, until the hold ends.
  setPeople([{ id: 'a', name: 'Ann renamed' }]);
  expect(list().map((entry) => [entry.key, entry.leaving()])).toEqual([['a', false], ['b', true]]);
  vi.advanceTimersByTime(139);
  expect(list()).toHaveLength(2);
  vi.advanceTimersByTime(1);
  expect(list().map((entry) => entry.key)).toEqual(['a']);

  // Carol arrives later: she is entering.
  setPeople([{ id: 'a', name: 'Ann renamed' }, { id: 'c', name: 'Carol' }]);
  expect(list().find((entry) => entry.key === 'c')?.entering).toBe(true);
  dispose();
});

it('createPresence with no hold removes at once', () => {
  const [items, setItems] = createSignal([{ id: 1 }, { id: 2 }]);
  let list!: ReturnType<typeof createPresence<{ id: number }>>;
  const dispose = createRoot((done) => {
    list = createPresence(items, (item) => String(item.id), 0);
    return done;
  });
  setItems([{ id: 1 }]);
  expect(list().map((entry) => entry.key)).toEqual(['1']);
  dispose();
});
