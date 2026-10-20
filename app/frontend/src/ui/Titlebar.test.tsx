import { render } from 'solid-js/web';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';

/** A stand-in for the Tauri window: the maximized flag lives here, and `resize()` plays what the OS does. */
const win = vi.hoisted(() => {
  const state = { maximized: false, handlers: new Set<() => void>(), unlisten: vi.fn(), toggleMaximize: vi.fn(async () => {}), minimize: vi.fn(async () => {}), close: vi.fn(async () => {}) };
  return state;
});
vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({
    isMaximized: async () => win.maximized,
    onResized: async (handler: () => void) => {
      win.handlers.add(handler);
      return () => { win.handlers.delete(handler); win.unlisten(); };
    },
    toggleMaximize: win.toggleMaximize,
    minimize: win.minimize,
    close: win.close,
  }),
}));

import { DEFAULT_PREFS, setPrefs } from '../stores/settingsStore';
import { Titlebar } from './Titlebar';

let dispose: (() => void) | undefined;
const flush = () => new Promise((resolve) => setTimeout(resolve, 0));
const mount = () => {
  const host = document.createElement('div');
  document.body.appendChild(host);
  dispose = render(() => <Titlebar />, host);
  return host;
};
const resize = async (maximized: boolean) => {
  win.maximized = maximized;
  win.handlers.forEach((handler) => handler());
  await flush();
};
const buttons = (host: ParentNode) => Array.from(host.querySelectorAll<HTMLButtonElement>('.wc button'));
const setLanguage = (language: 'ru' | 'en') => setPrefs({ ...DEFAULT_PREFS, appearance: { ...DEFAULT_PREFS.appearance, language } });

beforeEach(() => {
  (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
  win.maximized = false;
  win.handlers.clear();
  win.unlisten.mockClear();
  win.toggleMaximize.mockClear();
  setLanguage('en');
});
afterEach(() => {
  dispose?.();
  dispose = undefined;
  document.body.innerHTML = '';
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
});

it('gives each window button a hint equal to its label', async () => {
  const host = mount();
  await flush();
  expect(buttons(host).map((button) => [button.getAttribute('aria-label'), button.getAttribute('title')])).toEqual([
    ['Minimize', 'Minimize'], ['Maximize', 'Maximize'], ['Close', 'Close'],
  ]);
});

it('says Maximize in a normal window and Restore in a maximized one, from the start', async () => {
  win.maximized = true;
  const host = mount();
  await flush();
  expect(buttons(host)[1].getAttribute('aria-label')).toBe('Restore');
  expect(buttons(host)[1].getAttribute('title')).toBe('Restore');
});

it('follows the window as it is maximized and restored', async () => {
  const host = mount();
  await flush();
  expect(buttons(host)[1].getAttribute('aria-label')).toBe('Maximize');
  await resize(true);
  expect(buttons(host)[1].getAttribute('aria-label')).toBe('Restore');
  await resize(false);
  expect(buttons(host)[1].getAttribute('aria-label')).toBe('Maximize');
});

it('is translated, including the restore state', async () => {
  setLanguage('ru');
  const host = mount();
  await flush();
  expect(buttons(host).map((button) => button.getAttribute('title'))).toEqual(['Свернуть', 'Развернуть', 'Закрыть']);
  await resize(true);
  expect(buttons(host)[1].getAttribute('title')).toBe('Восстановить');
});

it('still toggles the window with the middle button and stops listening when removed', async () => {
  const host = mount();
  await flush();
  buttons(host)[1].click();
  await flush();
  expect(win.toggleMaximize).toHaveBeenCalledTimes(1);
  expect(win.handlers.size).toBe(1);
  dispose?.();
  dispose = undefined;
  await flush();
  expect(win.unlisten).toHaveBeenCalledTimes(1);
  expect(win.handlers.size).toBe(0);
});

it('does not touch the window API in a plain browser', async () => {
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  const host = mount();
  await flush();
  expect(buttons(host)).toHaveLength(0);
  expect(win.handlers.size).toBe(0);
});
