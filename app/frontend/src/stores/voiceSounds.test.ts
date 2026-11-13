import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { api } from '../services/api';
import { TONES } from './desktop';
import { sessionHandlers, setPresence } from './sessionStore';
import { setCurrentUser } from './authStore';
import { DEFAULT_PREFS, setPrefs, updateNotifications } from './settingsStore';
import {
  DEFAULT_VOICE_SETTINGS, handlePushToTalk, joinVoice, leaveVoice, setActiveVoiceChannel, setIsDeafened, setIsMuted, setVoiceSettings,
  setVoiceParticipants, toggleDeafen, toggleMute,
} from './voiceStore';
import { setAppError } from './notificationStore';

// The tones are synthesised: record every oscillator frequency so a test can tell which tone played.
const created: number[] = [];
vi.stubGlobal('AudioContext', class {
  currentTime = 0;
  destination = {};
  createOscillator() {
    const oscillator = {
      type: '',
      frequency: { set value(hz: number) { created.push(hz); }, get value() { return 0; } },
      connect: () => ({ connect: () => {} }),
      start() {},
      stop() {},
    };
    return oscillator;
  }
  createGain() {
    return {
      gain: { setValueAtTime() {}, linearRampToValueAtTime() {}, exponentialRampToValueAtTime() {} },
      connect: () => ({}),
    };
  }
  createBiquadFilter() {
    return { type: '', frequency: { value: 0 }, connect: () => ({}) };
  }
});

/** Frequencies of every note played since the last call. */
const played = () => created.splice(0);

const room = { id: 'ours', short_id: 1, name: 'ours', channel_type: 'voice' as const, position: 0, bitrate_kbps: 64 };
const me = { id: 'me', username: 'me', display_name: 'Me', role: 'member' as const };

beforeEach(() => {
  vi.restoreAllMocks();
  localStorage.clear();
  created.length = 0;
  // The soft set keeps the note values these assertions were written for.
  setPrefs({ ...DEFAULT_PREFS, notifications: { ...DEFAULT_PREFS.notifications, soundSet: 'soft' } });
  setCurrentUser(me);
  setPresence({});
  setAppError(null);
  setActiveVoiceChannel(room);
  setVoiceParticipants([]);
  setIsMuted(false);
  setIsDeafened(false);
  setVoiceSettings(DEFAULT_VOICE_SETTINGS);
  vi.spyOn(api, 'setVoiceState').mockResolvedValue();
});

afterEach(() => vi.restoreAllMocks());

it('the soft tones are the old notes a fourth lower', () => {
  expect(TONES.mute).toEqual({ notes: [390, 294], noteMs: 70 });
  expect(TONES.unmute).toEqual({ notes: [294, 390], noteMs: 70 });
  expect(TONES.deafen).toEqual({ notes: [330, 221], noteMs: 80 });
  expect(TONES.undeafen).toEqual({ notes: [221, 330], noteMs: 80 });
  expect(TONES['ptt-on']).toEqual({ notes: [555], noteMs: 45 });
  expect(TONES['ptt-off']).toEqual({ notes: [390], noteMs: 45 });
});

it('toggleMute plays mute, then unmute, once each', async () => {
  await toggleMute();
  expect(played()).toEqual(TONES.mute.notes);
  await toggleMute();
  expect(played()).toEqual(TONES.unmute.notes);
});

it('toggleDeafen plays only the deafen tone, and undeafen only the undeafen tone', async () => {
  await toggleDeafen();
  expect(played()).toEqual(TONES.deafen.notes);
  await toggleDeafen();
  expect(played()).toEqual(TONES.undeafen.notes);
});

it('unmuting while deafened plays only the undeafen tone', async () => {
  await toggleDeafen();
  played();
  await toggleMute();
  expect(played()).toEqual(TONES.undeafen.notes);
});

it('a failed mute change plays nothing', async () => {
  vi.spyOn(console, 'error').mockImplementation(() => {});
  vi.mocked(api.setVoiceState).mockRejectedValue(new Error('gone'));
  await expect(toggleMute()).rejects.toThrow();
  expect(played()).toEqual([]);
});

it('the hotkey event and the tray menu play the same mute tone as the button', async () => {
  sessionHandlers.hotkey('toggle_mute');
  await vi.waitFor(() => expect(played()).toEqual(TONES.mute.notes));
  sessionHandlers['tray-toggle-mute'](null);
  await vi.waitFor(() => expect(played()).toEqual(TONES.unmute.notes));
  sessionHandlers.hotkey('toggle_deafen');
  await vi.waitFor(() => expect(played()).toEqual(TONES.deafen.notes));
});

it('a moderator silencing me plays no mute tone', () => {
  sessionHandlers['server-mute-changed']({ peer_id: 1, server_muted: true });
  expect(played()).toEqual([]);
});

it('joining a room plays join after the join succeeds', async () => {
  setActiveVoiceChannel(null);
  vi.spyOn(api, 'joinVoiceChannel').mockResolvedValue(undefined as never);
  await joinVoice(room);
  expect(played()).toEqual(TONES.join.notes);
});

it('a failed join plays nothing', async () => {
  setActiveVoiceChannel(null);
  vi.spyOn(api, 'joinVoiceChannel').mockRejectedValue(new Error('Connection refused'));
  vi.spyOn(console, 'error').mockImplementation(() => {});
  await joinVoice(room);
  expect(played()).toEqual([]);
});

it('leaving a room plays leave after the leave succeeds', async () => {
  vi.spyOn(api, 'leaveVoiceChannel').mockResolvedValue(undefined as never);
  await leaveVoice();
  expect(played()).toEqual(TONES.leave.notes);
});

it('someone else leaving my room plays leave', () => {
  sessionHandlers['user-left-voice']({ user_id: 'bob', peer_id: 2, channel_id: 'ours' });
  expect(played()).toEqual(TONES.leave.notes);
});

it('someone leaving another room, or my own leaving event, plays nothing', () => {
  sessionHandlers['user-left-voice']({ user_id: 'bob', peer_id: 2, channel_id: 'theirs' });
  expect(played()).toEqual([]);
  sessionHandlers['user-left-voice']({ user_id: 'me', peer_id: 3, channel_id: 'ours' });
  expect(played()).toEqual([]);
});

it('push-to-talk plays ptt-on and ptt-off only in a room with push-to-talk on', () => {
  setVoiceSettings({ ...DEFAULT_VOICE_SETTINGS, push_to_talk: true });
  handlePushToTalk(true);
  expect(played()).toEqual(TONES['ptt-on'].notes);
  handlePushToTalk(false);
  expect(played()).toEqual(TONES['ptt-off'].notes);
});

it('push-to-talk plays nothing outside a room or with push-to-talk off', () => {
  setVoiceSettings({ ...DEFAULT_VOICE_SETTINGS, push_to_talk: true });
  setActiveVoiceChannel(null);
  handlePushToTalk(true);
  expect(played()).toEqual([]);
  setActiveVoiceChannel(room);
  setVoiceSettings({ ...DEFAULT_VOICE_SETTINGS, push_to_talk: false });
  handlePushToTalk(true);
  expect(played()).toEqual([]);
});

it('the push-to-talk event reaches the handler', () => {
  setVoiceSettings({ ...DEFAULT_VOICE_SETTINGS, push_to_talk: true });
  sessionHandlers['push-to-talk']({ pressed: true });
  expect(played()).toEqual(TONES['ptt-on'].notes);
});

it('voiceSounds off silences the feedback sounds', async () => {
  updateNotifications({ voiceSounds: false });
  setVoiceSettings({ ...DEFAULT_VOICE_SETTINGS, push_to_talk: true });
  vi.spyOn(api, 'joinVoiceChannel').mockResolvedValue(undefined as never);
  await toggleMute();
  await toggleDeafen();
  handlePushToTalk(true);
  expect(played()).toEqual([]);
});

it('someone leaving my room is governed by the voiceJoin setting, not voiceSounds', () => {
  updateNotifications({ voiceSounds: false });
  sessionHandlers['user-left-voice']({ user_id: 'bob', peer_id: 2, channel_id: 'ours' });
  expect(played()).toEqual(TONES.leave.notes);
  updateNotifications({ voiceJoin: false });
  sessionHandlers['user-left-voice']({ user_id: 'bob', peer_id: 2, channel_id: 'ours' });
  expect(played()).toEqual([]);
});

it('do not disturb does not silence the feedback sounds', async () => {
  setPresence({ me: 'donotdisturb' });
  setCurrentUser({ ...me, status: 'donotdisturb' });
  await toggleMute();
  expect(played()).toEqual(TONES.mute.notes);
});

it('voiceSounds defaults to on and is remembered', async () => {
  const { parsePrefs } = await import('./settingsStore');
  expect(DEFAULT_PREFS.notifications.voiceSounds).toBe(true);
  expect(parsePrefs(null).notifications.voiceSounds).toBe(true);
  expect(parsePrefs(JSON.stringify({ notifications: { voiceSounds: false } })).notifications.voiceSounds).toBe(false);
  expect(parsePrefs(JSON.stringify({ notifications: { voiceSounds: 'no' } })).notifications.voiceSounds).toBe(true);
});
