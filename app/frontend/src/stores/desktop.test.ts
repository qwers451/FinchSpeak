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
import { handleNoticeActivated, isDoNotDisturb, mentionCount, reactToFlockCall, reactToMessage, reactToShareStarted, startDesktopSync, trayStateNow } from './desktop';
import { sessionHandlers, setPresence } from './sessionStore';
import { setCurrentUser } from './authStore';
import { setChannels } from './chatStore';
import { setUnread } from './activityStore';
import { muteChannel, setPrefs, DEFAULT_PREFS, updateAppearance, updateNotifications } from './settingsStore';
import { setIsSharing } from './screenState';
import { activeVoiceChannel, setActiveVoiceChannel, setIsDeafened, setIsLocalSpeaking, setIsMuted, setVoiceParticipants } from './voiceStore';

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
  vi.unstubAllGlobals();
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

/** Pretend to be the Windows app: the picture-and-buttons path is only taken there. */
function asWindows() {
  vi.stubGlobal('navigator', { userAgent: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)' });
}

it('a click on a channel notice opens that channel', () => {
  handleNoticeActivated({ target: { kind: 'channel', id: 'c1' }, action: null });
  expect(selectChannel).toHaveBeenCalledWith(expect.objectContaining({ id: 'c1' }));
});

it('a reply to my message is a notice of its own, switched by Replies', async () => {
  asWindows();
  const notify = vi.spyOn(api, 'notify').mockResolvedValue();
  setCurrentUser({ ...me, status: 'online' });
  const reply = (author_id: string) => ({ ...message, mentions: [], reply_to: { message_id: 'm0', author_name: 'Me', snippet: 'x', author_id } });
  expect(reactToMessage(reply('me'), false, false)).toBe('notified');
  await vi.waitFor(() => expect(notify).toHaveBeenCalledTimes(1));
  expect(notify.mock.calls[0][0]).toMatchObject({ title: expect.stringContaining('#general'), target: { kind: 'channel', id: 'c1' }, avatar_user_id: 'other' });

  expect(reactToMessage(reply('someone-else'), false, false)).toBe('none');
  updateNotifications({ replies: false });
  expect(reactToMessage(reply('me'), false, false)).toBe('none');
  expect(notify).toHaveBeenCalledTimes(1);
});

it('a call to my room in the background offers Fly and Stay with the caller picture', async () => {
  asWindows();
  const notify = vi.spyOn(api, 'notify').mockResolvedValue();
  setCurrentUser({ ...me, status: 'online' });
  reactToFlockCall({ from_user_id: 'bob', from_name: 'Bob', channel_id: 'v' }, false);
  await vi.waitFor(() => expect(notify).toHaveBeenCalledTimes(1));
  expect(notify.mock.calls[0][0]).toMatchObject({
    avatar_user_id: 'bob',
    target: { kind: 'voice_room', id: 'v' },
    actions: [{ id: 'fly', label: 'Лететь' }, { id: 'stay', label: 'Остаться' }],
  });
});

it('no call notice in Do not disturb, in a muted room or while the window is focused', () => {
  asWindows();
  const notify = vi.spyOn(api, 'notify').mockResolvedValue();
  const call = { from_user_id: 'bob', from_name: 'Bob', channel_id: 'v' };
  setCurrentUser({ ...me, status: 'donotdisturb' });
  reactToFlockCall(call, false);
  setCurrentUser({ ...me, status: 'online' });
  reactToFlockCall(call, true);
  muteChannel('v', null);
  reactToFlockCall(call, false);
  expect(notify).not.toHaveBeenCalled();
});

it('Fly when I am already in that room changes nothing', async () => {
  setChannels([voiceRoom as never]);
  setActiveVoiceChannel(voiceRoom);
  const join = vi.spyOn(api, 'joinVoiceChannel').mockResolvedValue(undefined as never);
  handleNoticeActivated({ target: { kind: 'voice_room', id: 'v' }, action: 'fly' });
  await vi.waitFor(() => expect(join).not.toHaveBeenCalled());
  expect(activeVoiceChannel()?.id).toBe('v');
});

it('a screen share starts a notice only when it is in my room', async () => {
  asWindows();
  const notify = vi.spyOn(api, 'notify').mockResolvedValue();
  setCurrentUser({ ...me, status: 'online' });
  setActiveVoiceChannel(voiceRoom);
  setVoiceParticipants([
    { user_id: 'bob', peer_id: 2, username: 'Bob', channel_id: 'v' },
    { user_id: 'ann', peer_id: 3, username: 'Ann', channel_id: 'elsewhere' },
  ] as never);
  reactToShareStarted('ann', false);
  expect(notify).not.toHaveBeenCalled();
  reactToShareStarted('bob', false);
  await vi.waitFor(() => expect(notify).toHaveBeenCalledTimes(1));
  expect(notify.mock.calls[0][0]).toMatchObject({ avatar_user_id: 'bob', target: { kind: 'voice_room', id: 'v' } });
});

it('dnd_from_presence_survives_sign_in_without_status_field', () => {
  // The self presence-update arrives before setCurrentUser and the user has no status field.
  sessionHandlers['presence-update']({ user_id: 'me', status: 'donotdisturb' });
  setCurrentUser({ id: 'me', username: 'me', display_name: 'Me', role: 'member' });
  expect(isDoNotDisturb()).toBe(true);
  setPresence({});
});
