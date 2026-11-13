import { createEffect, createSignal, onCleanup, type Accessor } from 'solid-js';

/**
 * Shared motion tokens. The numbers mirror the `--motion-*` custom properties in `styles/motion.css`
 * and `--ease-out` in `styles/themes.css`; `motion.test.ts` keeps them equal.
 */
export const MOTION = {
  /** Hover, press and thumb feedback. */
  small: 120,
  /** Panels, menus, a new message sliding in, layout moves. */
  panel: 160,
  base: 180,
  /** Entrances of cards and modals. */
  enter: 220,
  pop: 240,
  reveal: 260,
  /** Exits are a little quicker than entrances. */
  exit: 140,
  /** Delay between the items of one wave. */
  stagger: 30,
  /** A wave never staggers more items than this. */
  maxStagger: 8,
} as const;

export const EASE = {
  out: 'cubic-bezier(.16,1,.3,1)',
  in: 'cubic-bezier(.4,0,1,1)',
  /** Light overshoot: only for small pop elements (reactions, badges) and card entries. */
  spring: 'cubic-bezier(.34,1.56,.64,1)',
} as const;

/** True when the app setting or the system asks for less motion. */
export function prefersReducedMotion(): boolean {
  if (typeof document === 'undefined') return false;
  if (document.documentElement.dataset.reduceMotion === 'true') return true;
  return typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches;
}

/** Delay for item `index` of a wave: 30 ms per item; the eighth item and later share one delay. */
export function staggerDelay(index: number): number {
  return Math.min(Math.max(index, 0), MOTION.maxStagger - 1) * MOTION.stagger;
}

const canAnimate = (element: Element) => typeof element.animate === 'function' && !prefersReducedMotion();

/** Positions measured by the previous call, per container. */
const layouts = new WeakMap<HTMLElement, Map<string, DOMRect>>();

/**
 * FLIP for a reorderable container. Children with `data-flip="<key>"` are measured now; the positions
 * measured by the previous call are the "before" state. Children that moved glide from their old place
 * (transform only). Call it right after the DOM order changed. Returns how many children were animated.
 * Under reduced motion nothing animates: the final positions are already in place.
 */
export function animateLayout(container: HTMLElement): number {
  const tiles = [...container.querySelectorAll<HTMLElement>('[data-flip]')];
  const current = new Map<string, DOMRect>();
  for (const tile of tiles) current.set(tile.dataset.flip ?? '', tile.getBoundingClientRect());
  const previous = layouts.get(container);
  layouts.set(container, current);
  if (!previous) return 0;
  let moved = 0;
  for (const tile of tiles) {
    const key = tile.dataset.flip ?? '';
    const from = previous.get(key);
    const to = current.get(key);
    if (!from || !to || !canAnimate(tile)) continue;
    const dx = from.left - to.left;
    const dy = from.top - to.top;
    if (Math.abs(dx) < 0.5 && Math.abs(dy) < 0.5) continue;
    tile.animate([{ transform: `translate(${dx}px, ${dy}px)` }, { transform: 'translate(0, 0)' }], { duration: MOTION.base, easing: EASE.out });
    moved++;
  }
  return moved;
}

/** A small pop for a counter or reaction that grew: 0.6 -> 1.1 -> 1, with a light overshoot. */
export function popIn(element: Element, from = 0.6): Animation | undefined {
  if (!canAnimate(element)) return undefined;
  return element.animate(
    [{ transform: `scale(${from})` }, { transform: 'scale(1.1)', offset: 0.6 }, { transform: 'scale(1)' }],
    { duration: MOTION.pop, easing: 'linear' },
  );
}

/** A changed number slides in from above (direction 1) or from below (-1). */
export function rollIn(element: Element, direction: 1 | -1 = 1): Animation | undefined {
  if (!canAnimate(element)) return undefined;
  return element.animate(
    [{ transform: `translateY(${direction * 40}%)`, opacity: 0 }, { transform: 'translateY(0)', opacity: 1 }],
    { duration: MOTION.panel, easing: EASE.out },
  );
}

/** A soft ring grows from a click point inside `host` and fades. The host needs position: relative. */
export function rippleAt(host: HTMLElement, x: number, y: number): void {
  if (!canAnimate(host)) return;
  const dot = document.createElement('span');
  dot.className = 'ripple-dot';
  dot.setAttribute('aria-hidden', 'true');
  dot.style.left = `${x}px`;
  dot.style.top = `${y}px`;
  host.append(dot);
  const animation = dot.animate(
    [{ transform: 'translate(-50%, -50%) scale(0)', opacity: 0.6 }, { transform: 'translate(-50%, -50%) scale(1)', opacity: 0 }],
    { duration: MOTION.enter, easing: EASE.out },
  );
  animation.onfinish = () => dot.remove();
}

type ViewTransitionDocument = Document & { startViewTransition?: (update: () => void) => unknown };

/** Where the theme circle grows from: the last click (or the middle of the control for keyboard use). */
export function rememberRevealOrigin(x: number, y: number): void {
  if (typeof document === 'undefined') return;
  const root = document.documentElement;
  root.style.setProperty('--reveal-x', `${Math.round(x)}px`);
  root.style.setProperty('--reveal-y', `${Math.round(y)}px`);
}

if (typeof document !== 'undefined') {
  document.addEventListener('click', (event) => {
    const target = event.target instanceof Element ? event.target : null;
    if (event.detail === 0 && target) {
      const rect = target.getBoundingClientRect();
      rememberRevealOrigin(rect.left + rect.width / 2, rect.top + rect.height / 2);
    } else {
      rememberRevealOrigin(event.clientX, event.clientY);
    }
  }, true);
}

/**
 * Runs `paint` (the theme change) inside a View Transition, so the new theme opens as a circle from the
 * click point. Without the API, or under reduced motion, `paint` runs at once. Returns true when deferred.
 */
export function revealTheme(paint: () => void, reduceMotion = false): boolean {
  const doc = document as ViewTransitionDocument;
  if (typeof doc.startViewTransition !== 'function' || reduceMotion || prefersReducedMotion()) {
    paint();
    return false;
  }
  doc.startViewTransition(paint);
  return true;
}

export type PresenceEntry<T> = {
  key: string;
  item: Accessor<T>;
  /** Added after the first render: it should play its enter animation. */
  entering: boolean;
  /** Removed, kept for one exit animation. */
  leaving: Accessor<boolean>;
};

type StoredEntry<T> = PresenceEntry<T> & {
  setItem: (item: T) => void;
  setLeaving: (value: boolean) => void;
  /** Plain flag for the bookkeeping (reading the signal inside the effect would re-run it). */
  gone: boolean;
  timer?: ReturnType<typeof setTimeout>;
};

/**
 * Keeps removed items for `hold` ms so they can play an exit, and gives each key one stable entry: a list
 * rendered with `<For each={...}>` keeps its DOM node (and does not replay the enter animation) when the
 * item object is replaced. Items of the first value are not "entering". `hold` 0 removes at once.
 */
export function createPresence<T>(source: Accessor<T[]>, key: (item: T) => string, hold: number = MOTION.exit): Accessor<PresenceEntry<T>[]> {
  const entries = new Map<string, StoredEntry<T>>();
  const [list, setList] = createSignal<PresenceEntry<T>[]>([]);
  let first = true;

  createEffect(() => {
    const next = source();
    const seen = new Set<string>();
    const out: PresenceEntry<T>[] = [];
    for (const item of next) {
      const id = key(item);
      seen.add(id);
      let entry = entries.get(id);
      if (!entry) {
        const [current, setItem] = createSignal<T>(item);
        const [leaving, setLeaving] = createSignal(false);
        entry = { key: id, item: current, entering: !first, leaving, setItem, setLeaving, gone: false };
        entries.set(id, entry);
      } else {
        entry.setItem(item);
        if (entry.timer !== undefined) { clearTimeout(entry.timer); entry.timer = undefined; }
        if (entry.gone) { entry.gone = false; entry.setLeaving(false); }
      }
      out.push(entry);
    }
    for (const [id, entry] of entries) {
      if (seen.has(id)) continue;
      if (hold <= 0) { entries.delete(id); continue; }
      if (!entry.gone) {
        entry.gone = true;
        entry.setLeaving(true);
        entry.timer = setTimeout(() => {
          entries.delete(id);
          setList((current) => current.filter((candidate) => candidate.key !== id));
        }, hold);
      }
      out.push(entry);
    }
    first = false;
    setList(out);
  });
  onCleanup(() => { for (const entry of entries.values()) if (entry.timer !== undefined) clearTimeout(entry.timer); });
  return list;
}

/** The number of messages (or items) one enter wave may stagger. */
export const MAX_WAVE_ITEMS = MOTION.maxStagger;

/** The ring for a button press: from the pointer, or from the middle of the button for keyboard activation. */
export function rippleFromEvent(event: MouseEvent): void {
  const host = event.currentTarget;
  if (!(host instanceof HTMLElement)) return;
  const rect = host.getBoundingClientRect();
  if (event.detail === 0) rippleAt(host, rect.width / 2, rect.height / 2);
  else rippleAt(host, event.clientX - rect.left, event.clientY - rect.top);
}

/** Whether a theme switch may run as a circular reveal: the View Transitions API exists and motion is allowed. */
export function canRevealTheme(reduceMotion = false): boolean {
  if (typeof document === 'undefined' || reduceMotion || prefersReducedMotion()) return false;
  return typeof (document as ViewTransitionDocument).startViewTransition === 'function';
}
