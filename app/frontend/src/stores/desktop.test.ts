import { afterEach, beforeEach, expect, it, vi } from 'vitest';

const selectChannel = vi.fn(async () => {});
vi.mock('./chatStore', async (importOriginal) => ({
  ...(await importOriginal<typeof import('./chatStore')>()),
  selectChannel: (...args: unknown[]) => (selectChannel as (...a: unknown[]) => unknown)(...args),
}));

const sendNotification = vi.fn();
vi.mock('@tauri-apps/plugin-notification', () => ({
  isPermissionGranted: async () => true,
  requestPermission: async () => 'granted',
  sendNotification: (...args: unknown[]) => sendNotification(...args),
}));

import { mentionCount, reactToMessage, startDesktopSync } from './desktop';
import { setCurrentUser } from './authStore';
import { setChannels } from './chatStore';
import { setUnread } from './activityStore';
import { muteChannel, setPrefs, DEFAULT_PREFS } from './settingsStore';

const message = { channel_id: 'c1', author_id: 'other', author_name: 'Other', content: 'hi @me', mentions: [] };
const me = { id: 'me', username: 'me', display_name: 'Me', role: 'member' as const };

beforeEach(() => {
  sendNotification.mockClear();
  selectChannel.mockClear();
  setPrefs(DEFAULT_PREFS);
  setUnread({});
  setChannels([{ id: 'c1', name: 'general' } as never]);
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

afterEach(() => vi.useRealTimers());

it('mentionCount ignores muted channels', () => {
  setUnread({
    a: { count: 5, mention: true, mentions: 2 },
    b: { count: 1, mention: true },
    c: { count: 3, mention: false },
    d: { count: 4, mention: true, mentions: 3 },
  });
  muteChannel('d', null);
  expect(mentionCount()).toBe(3);
});

it('focus_soon_after_notification_jumps', async () => {
  vi.useFakeTimers();
  startDesktopSync();
  setCurrentUser({ ...me, status: 'online' });
  expect(reactToMessage(message, true, false)).toBe('notified');
  await vi.waitFor(() => expect(sendNotification).toHaveBeenCalled());
  await vi.advanceTimersByTimeAsync(5_000);
  window.dispatchEvent(new Event('focus'));
  expect(selectChannel).toHaveBeenCalledTimes(1);
  window.dispatchEvent(new Event('focus'));
  expect(selectChannel).toHaveBeenCalledTimes(1);
});

it('late_focus_does_not_jump', async () => {
  vi.useFakeTimers();
  startDesktopSync();
  setCurrentUser({ ...me, status: 'online' });
  expect(reactToMessage(message, true, false)).toBe('notified');
  await vi.waitFor(() => expect(sendNotification).toHaveBeenCalled());
  await vi.advanceTimersByTimeAsync(3 * 60_000);
  window.dispatchEvent(new Event('focus'));
  expect(selectChannel).not.toHaveBeenCalled();
});
