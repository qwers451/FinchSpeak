import { beforeEach, expect, it, vi } from 'vitest';
import { api } from '../services/api';
import { appError, setAppError } from './notificationStore';
import { isSharing, loadScreenSources, screenAudioKey, screenSources, startSharing, stopSharing, stopWatching, watchShare, watching } from './screenStore';
import { sessionHandlers, resetSessionState } from './sessionStore';
import { allVoiceParticipants, setVoiceParticipants } from './voiceStore';

beforeEach(() => {
  vi.restoreAllMocks();
  resetSessionState();
  setAppError(null);
  setVoiceParticipants([
    { user_id: 'alice', peer_id: 20, username: 'alice', channel_id: 'room' },
    { user_id: 'bob', peer_id: 21, username: 'bob', channel_id: 'room' },
  ]);
});

it('AUD-018 lists the screens and windows that can be shared', async () => {
  const sources = { monitors: [{ id: 1, name: 'DISPLAY1', width: 1920, height: 1080, is_primary: true }],
    windows: [{ id: 9, title: 'Notes', app_name: 'notepad', width: 800, height: 600 }] };
  vi.spyOn(api, 'listScreenSources').mockResolvedValue(sources);
  await loadScreenSources();
  expect(screenSources()).toEqual(sources);
});

it('AUD-018 marks us as sharing only when the backend accepted the share', async () => {
  vi.spyOn(api, 'startScreenShare').mockRejectedValue('Could not capture the screen');
  await startSharing('monitor', 1);
  expect(isSharing()).toBe(false);
  expect(appError()).toContain('Could not capture');

  vi.spyOn(api, 'startScreenShare').mockResolvedValue();
  await startSharing('monitor', 1);
  expect(isSharing()).toBe(true);

  const stop = vi.spyOn(api, 'stopScreenShare').mockResolvedValue();
  await stopSharing();
  expect(isSharing()).toBe(false);
  expect(stop).toHaveBeenCalled();
});

it('AUD-018 a share that ends by itself is reported and clears the sharing state', async () => {
  vi.spyOn(api, 'startScreenShare').mockResolvedValue();
  await startSharing('window', 9);
  sessionHandlers['screen-share-ended']('window closed');
  expect(isSharing()).toBe(false);
  expect(appError()).toContain('window closed');
});

it('AUD-018 shows who is sharing, and stops watching when they stop', async () => {
  sessionHandlers['screen-share-started']({ user_id: 'alice', peer_id: 20, width: 1280, height: 720, fps: 30 });
  expect(allVoiceParticipants().find((p) => p.peer_id === 20)?.is_sharing).toBe(true);
  expect(allVoiceParticipants().find((p) => p.peer_id === 21)?.is_sharing).toBeFalsy();

  const subscribe = vi.spyOn(api, 'subscribeScreenShare').mockResolvedValue();
  await watchShare('alice', 20);
  expect(subscribe).toHaveBeenCalledWith('alice', 20);
  expect(watching()).toEqual({ user_id: 'alice', peer_id: 20 });

  sessionHandlers['screen-share-stopped']({ peer_id: 20 });
  expect(allVoiceParticipants().find((p) => p.peer_id === 20)?.is_sharing).toBe(false);
  expect(watching()).toBeNull();
});

it('AUD-018 watching another share first leaves the previous one', async () => {
  vi.spyOn(api, 'subscribeScreenShare').mockResolvedValue();
  const unsubscribe = vi.spyOn(api, 'unsubscribeScreenShare').mockResolvedValue();
  await watchShare('alice', 20);
  await watchShare('bob', 21);
  expect(unsubscribe).toHaveBeenCalledWith('alice');
  expect(watching()?.peer_id).toBe(21);

  await stopWatching();
  expect(unsubscribe).toHaveBeenLastCalledWith('bob');
  expect(watching()).toBeNull();
});

it('AUD-018 someone who leaves the room stops being watched', async () => {
  vi.spyOn(api, 'subscribeScreenShare').mockResolvedValue();
  await watchShare('alice', 20);
  sessionHandlers['user-left-voice']({ user_id: 'alice', peer_id: 20, channel_id: 'room' });
  expect(watching()).toBeNull();
});

it('AUD-018 losing the connection ends our share but keeps what we watch', async () => {
  vi.spyOn(api, 'startScreenShare').mockResolvedValue();
  vi.spyOn(api, 'subscribeScreenShare').mockResolvedValue();
  await startSharing('monitor', 1);
  await watchShare('alice', 20);
  sessionHandlers['connection-lost']('network down');
  expect(isSharing()).toBe(false);
  expect(watching()).toEqual({ user_id: 'alice', peer_id: 20 });
});

const snapshot = (voice_states: { user_id: string; peer_id: number; sharing_screen: boolean }[]) => ({
  user_id: 'me', session_peer_id: 10, channels: [], initial_state: { channels: [], online_users: [],
    voice_states: voice_states.map((state) => ({ ...state, username: state.user_id, channel_id: 'room', muted: false, deafened: false })) },
});

it('AUD-018 after a reconnect the watched share continues if its sharer is still sharing', async () => {
  vi.spyOn(api, 'subscribeScreenShare').mockResolvedValue();
  await watchShare('alice', 20);
  sessionHandlers['connection-lost']('network down');
  sessionHandlers.authenticated(snapshot([{ user_id: 'alice', peer_id: 33, sharing_screen: true }]));
  expect(watching()).toEqual({ user_id: 'alice', peer_id: 33 });
});

it('AUD-018 after a reconnect a share that ended meanwhile is closed', async () => {
  vi.spyOn(api, 'subscribeScreenShare').mockResolvedValue();
  await watchShare('alice', 20);
  sessionHandlers['connection-lost']('network down');
  sessionHandlers.authenticated(snapshot([{ user_id: 'alice', peer_id: 20, sharing_screen: false }]));
  expect(watching()).toBeNull();
});

it('AUD-018 a restarted share keeps being watched under its new peer id', async () => {
  vi.spyOn(api, 'subscribeScreenShare').mockResolvedValue();
  await watchShare('alice', 20);
  sessionHandlers['screen-share-started']({ user_id: 'alice', peer_id: 44, width: 1280, height: 720, fps: 30 });
  expect(watching()).toEqual({ user_id: 'alice', peer_id: 44 });
});

it('AUD-018 the snapshot tells a late joiner who is already sharing', () => {
  sessionHandlers.authenticated({ user_id: 'me', session_peer_id: 10, channels: [], initial_state: {
    channels: [], online_users: [],
    voice_states: [{ user_id: 'alice', peer_id: 20, username: 'alice', channel_id: 'room', muted: false, deafened: false, sharing_screen: true }],
  } });
  expect(allVoiceParticipants()[0].is_sharing).toBe(true);
});

it('AUD-018 the screen sound is requested only when asked for, and failures are shown', async () => {
  const start = vi.spyOn(api, 'startScreenShare').mockResolvedValue();
  await startSharing('monitor', 1);
  expect(start).toHaveBeenLastCalledWith('monitor', 1, false);
  await startSharing('monitor', 1, true);
  expect(start).toHaveBeenLastCalledWith('monitor', 1, true);
  sessionHandlers['screen-audio-failed']('no output device');
  expect(appError()).toContain('no output device');
});

it('AUD-018 the screen sound has its own volume key, apart from the voice', () => {
  expect(screenAudioKey(7)).toBe(7 + 0x40000000);
  expect(screenAudioKey(7)).not.toBe(7);
});
