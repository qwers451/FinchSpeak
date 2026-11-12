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

import { api } from '../services/api';
import { isDoNotDisturb, mentionCount, reactToMessage, startDesktopSync, trayStateNow } from './desktop';
import { sessionHandlers, setPresence } from './sessionStore';
import { setCurrentUser } from './authStore';
import { setChannels } from './chatStore';
import { setUnread } from './activityStore';
import { muteChannel, setPrefs, DEFAULT_PREFS, updateAppearance, updateNotifications } from './settingsStore';
import { setIsSharing } from './screenState';
import { setActiveVoiceChannel, setIsDeafened, setIsLocalSpeaking, setIsMuted } from './voiceStore';

const message = { channel_id: 'c1', author_id: 'other', author_name: 'Other', content: 'hi @me', mentions: [] };
const me = { id: 'me', username: 'me', display_name: 'Me', role: 'member' as const };
const voiceRoom = { id: 'v', short_id: 1, name: 'Voice', channel_type: 'voice' as const, position: 0, bitrate_kbps: 64 };

beforeEach(() => {
  sendNotification.mockClear();
  selectChannel.mockClear();
  setPrefs(DEFAULT_PREFS);
  setUnread({});
  setChannels([{ id: 'c1', name: 'general' } as never]);
  setActiveVoiceChannel(null);
  setIsDeafened(false);
  setIsLocalSpeaking(false);
  setIsMuted(false);
  setIsSharing(false);
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

afterEach(() => {
  vi.restoreAllMocks();
  vi.useRealTimers();
});

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

it('the tray prioritizes screen sharing and mic mute over local speech', () => {
  setIsLocalSpeaking(true);
  expect(trayStateNow()).toBe('idle');
  setActiveVoiceChannel(voiceRoom);
  expect(trayStateNow()).toBe('speaking');
  setIsDeafened(true);
  expect(trayStateNow()).toBe('muted');
  setIsMuted(true);
  setIsDeafened(false);
  expect(trayStateNow()).toBe('muted');
  setIsSharing(true);
  expect(trayStateNow()).toBe('live');
});

it('three syllables with 200 ms pauses produce one speaking update and one release', () => {
  vi.useFakeTimers();
  const setTrayState = vi.spyOn(api, 'setTrayState').mockResolvedValue();
  startDesktopSync();
  setActiveVoiceChannel(voiceRoom);
  expect(setTrayState).toHaveBeenLastCalledWith('voice', 'ru');
  setTrayState.mockClear();

  for (let syllable = 0; syllable < 3; syllable++) {
    setIsLocalSpeaking(true);
    expect(setTrayState.mock.calls).toEqual([['speaking', 'ru']]);
    vi.advanceTimersByTime(100);
    setIsLocalSpeaking(false);
    vi.advanceTimersByTime(200);
  }
  updateNotifications({ sounds: false });
  vi.advanceTimersByTime(199);
  expect(setTrayState.mock.calls).toEqual([['speaking', 'ru']]);

  vi.advanceTimersByTime(1);
  expect(setTrayState.mock.calls).toEqual([['speaking', 'ru'], ['voice', 'ru']]);
});

it('muting during a short silence cancels the pending speaking release', () => {
  vi.useFakeTimers();
  const setTrayState = vi.spyOn(api, 'setTrayState').mockResolvedValue();
  startDesktopSync();
  setActiveVoiceChannel(voiceRoom);
  setIsLocalSpeaking(true);
  setIsLocalSpeaking(false);
  setTrayState.mockClear();

  setIsMuted(true);
  vi.advanceTimersByTime(400);
  expect(setTrayState.mock.calls).toEqual([['muted', 'ru']]);
});

it('changing the interface language refreshes the current native tray copy', () => {
  const setTrayState = vi.spyOn(api, 'setTrayState').mockResolvedValue();
  startDesktopSync();
  setActiveVoiceChannel(voiceRoom);
  setTrayState.mockClear();

  updateAppearance({ language: 'en' });
  expect(setTrayState.mock.calls).toEqual([['voice', 'en']]);
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

it('dnd_from_presence_survives_sign_in_without_status_field', () => {
  // The self presence-update arrives before setCurrentUser and the user has no status field.
  sessionHandlers['presence-update']({ user_id: 'me', status: 'donotdisturb' });
  setCurrentUser({ id: 'me', username: 'me', display_name: 'Me', role: 'member' });
  expect(isDoNotDisturb()).toBe(true);
  setPresence({});
});
