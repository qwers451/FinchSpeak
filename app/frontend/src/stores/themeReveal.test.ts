import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { applyAppearance, DEFAULT_PREFS } from './settingsStore';

type ViewDocument = Document & { startViewTransition?: (update: () => void) => unknown };

const appearance = (theme: 'claude' | 'custom', reduceMotion = false) => ({ ...DEFAULT_PREFS.appearance, theme, reduceMotion });

beforeEach(() => {
  Reflect.deleteProperty(document, 'startViewTransition');
  document.documentElement.removeAttribute('data-reduce-motion');
  applyAppearance(appearance('claude'));
});

afterEach(() => {
  Reflect.deleteProperty(document, 'startViewTransition');
  applyAppearance(appearance('claude'));
});

it('switches the theme at once when the View Transitions API is missing', () => {
  applyAppearance(appearance('custom'));
  expect(document.documentElement.dataset.theme).toBe('custom');
});

it('opens the new theme inside a view transition when the API exists', () => {
  const start = vi.fn();
  (document as ViewDocument).startViewTransition = start;
  applyAppearance(appearance('custom'));
  expect(start).toHaveBeenCalledTimes(1);
  // Until the browser runs the update, the old theme is still on the page.
  expect(document.documentElement.dataset.theme).toBe('claude');
  start.mock.calls[0][0]();
  expect(document.documentElement.dataset.theme).toBe('custom');
});

it('does not start a transition under reduced motion', () => {
  const start = vi.fn();
  (document as ViewDocument).startViewTransition = start;
  applyAppearance(appearance('custom', true));
  expect(start).not.toHaveBeenCalled();
  expect(document.documentElement.dataset.theme).toBe('custom');
});

it('does not start a transition for a change that is not a theme switch', () => {
  const start = vi.fn();
  (document as ViewDocument).startViewTransition = start;
  applyAppearance(appearance('claude'));
  applyAppearance({ ...appearance('claude'), density: 'compact' });
  expect(start).not.toHaveBeenCalled();
});
