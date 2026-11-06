import { beforeEach, expect, it, vi } from 'vitest';

const sendNotification = vi.fn();
vi.mock('@tauri-apps/plugin-notification', () => ({
  isPermissionGranted: async () => true,
  requestPermission: async () => 'granted',
  sendNotification: (...args: unknown[]) => sendNotification(...args),
}));

import { reactToMessage } from './desktop';
import { setCurrentUser } from './authStore';

const message = { channel_id: 'c1', author_id: 'other', author_name: 'Other', content: 'hi @me', mentions: [] };
const me = { id: 'me', username: 'me', display_name: 'Me', role: 'member' as const };

beforeEach(() => {
  sendNotification.mockClear();
  (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
});

it('dnd_keeps_unread_but_stays_silent', async () => {
  setCurrentUser({ ...me, status: 'donotdisturb' });
  expect(reactToMessage(message, true, false)).toBe('none');
  await new Promise((resolve) => setTimeout(resolve, 0));
  expect(sendNotification).not.toHaveBeenCalled();
});

it('online user is notified', async () => {
  setCurrentUser({ ...me, status: 'online' });
  expect(reactToMessage(message, true, false)).toBe('notified');
  await vi.waitFor(() => expect(sendNotification).toHaveBeenCalled());
});
