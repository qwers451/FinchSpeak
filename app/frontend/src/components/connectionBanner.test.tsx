import { render } from 'solid-js/web';
import { afterEach, expect, it, vi } from 'vitest';
import { setCurrentUser } from '../stores/authStore';
import { setConnectionStatus } from '../stores/sessionStore';
import type { User } from '../types';

const mocks = vi.hoisted(() => ({ reconnectNow: vi.fn(async () => {}), logoutUser: vi.fn(async () => {}) }));
vi.mock('../services/api', async (importOriginal) => {
  const original = await importOriginal<typeof import('../services/api')>();
  return { ...original, api: { ...original.api, reconnectNow: mocks.reconnectNow } };
});
vi.mock('../stores/authStore', async (importOriginal) => ({ ...(await importOriginal<typeof import('../stores/authStore')>()), logoutUser: mocks.logoutUser }));

import { ConnectionBanner } from './ConnectionBanner';

const user: User = { id: 'u', username: 'a', display_name: 'A', role: 'member', avatar_url: null };
let dispose: (() => void) | undefined;

afterEach(() => {
  dispose?.();
  dispose = undefined;
  document.body.innerHTML = '';
  setCurrentUser(null);
  setConnectionStatus('disconnected');
  vi.clearAllMocks();
});

const button = (label: string) => [...document.querySelectorAll('button')].find((b) => b.textContent?.trim() === label);

it('offers "Retry now" while reconnecting and calls the backend', () => {
  setCurrentUser(user);
  setConnectionStatus('reconnecting');
  dispose = render(() => <ConnectionBanner />, document.body);
  const retry = button('Retry now');
  expect(retry).toBeTruthy();
  retry!.click();
  expect(mocks.reconnectNow).toHaveBeenCalledTimes(1);
});

it('offers "Back to sign in" once reconnecting has failed', () => {
  setCurrentUser(user);
  setConnectionStatus('failed');
  dispose = render(() => <ConnectionBanner />, document.body);
  expect(button('Retry now')).toBeUndefined();
  button('Back to sign in')!.click();
  expect(mocks.logoutUser).toHaveBeenCalledTimes(1);
});

it.each(['Session revoked', 'Invalid token'])('the connection error %s is shown in the interface language', async (reason) => {
  const { setConnectionError } = await import('../stores/sessionStore');
  const { DEFAULT_PREFS, setPrefs } = await import('../stores/settingsStore');
  const translated = reason === 'Session revoked' ? 'Сеанс отозван' : 'Недействительный токен';
  setPrefs({ ...DEFAULT_PREFS, appearance: { ...DEFAULT_PREFS.appearance, language: 'ru' } });
  setCurrentUser(user);
  setConnectionError(reason);
  setConnectionStatus('failed');
  dispose = render(() => <ConnectionBanner />, document.body);
  expect(document.body.textContent).toContain(translated);
  expect(document.body.textContent).not.toContain(reason);
  setConnectionError(null);
  setPrefs(DEFAULT_PREFS);
});

it('the generic "connection lost" is not repeated as a reason under the headline', async () => {
  const { setConnectionError } = await import('../stores/sessionStore');
  const { DEFAULT_PREFS, setPrefs } = await import('../stores/settingsStore');
  setPrefs({ ...DEFAULT_PREFS, appearance: { ...DEFAULT_PREFS.appearance, language: 'ru' } });
  setCurrentUser(user);
  setConnectionError('connection lost');
  setConnectionStatus('failed');
  dispose = render(() => <ConnectionBanner />, document.body);
  expect(document.body.textContent).toContain('Не удалось восстановить соединение');
  expect(document.body.textContent).not.toContain('Связь потеряна');
  setConnectionError(null);
  setPrefs(DEFAULT_PREFS);
});
