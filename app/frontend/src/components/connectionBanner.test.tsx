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
