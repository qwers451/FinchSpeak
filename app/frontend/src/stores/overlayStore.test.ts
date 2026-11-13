import { beforeEach, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import type { User, VoiceParticipant } from '../types';
import { setGameActivity } from './activityGameStore';
import { parsePrefs, DEFAULT_PREFS, prefs, setPrefs } from './settingsStore';
import { setSessionUserId } from './sessionStore';
import {
  applyAutoOverlay, autoOverlayAction, buildOverlayState, hideOverlay, overlayOpen, setOverlayOpen, setOverlayPref, showOverlay,
} from './overlayStore';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn().mockResolvedValue(undefined) }));
vi.mock('@tauri-apps/api/event', () => ({ emitTo: vi.fn().mockResolvedValue(undefined), listen: vi.fn().mockResolvedValue(() => undefined) }));

const ME = '11111111-1111-1111-1111-111111111111';
const ANNA = '22222222-2222-2222-2222-222222222222';
const users: User[] = [
  { id: ME, username: 'alice', display_name: 'Alice', role: 'member', avatar_url: null },
  { id: ANNA, username: 'anna', display_name: '', role: 'member', avatar_url: 'https://example.com/a.png' },
];
const participant = (overrides: Partial<VoiceParticipant>): VoiceParticipant => ({
  user_id: ME, peer_id: 1, username: 'alice', channel_id: 'c1', ...overrides,
});
const input = (overrides: Partial<Parameters<typeof buildOverlayState>[0]> = {}) => ({
  room: { name: 'Lobby' },
  participants: [participant({}), participant({ user_id: ANNA, peer_id: 2, username: 'anna', is_muted: true })],
  users,
  localPeerId: 1,
  speakingPeers: new Set<number>([2]),
  localSpeaking: false,
  localMuted: false,
  ...overrides,
});

beforeEach(async () => {
  vi.mocked(invoke).mockClear();
  localStorage.clear();
  setPrefs({ ...DEFAULT_PREFS, overlay: { ...DEFAULT_PREFS.overlay, autoInGame: false } });
  setSessionUserId(ME);
  setGameActivity({ user_id: ME, game: null });
  setOverlayOpen(false);
  await hideOverlay();
  vi.mocked(invoke).mockClear();
});

it('the state lists the people of the room with names, avatars, speaking and muted', () => {
  const state = buildOverlayState(input());
  expect(state.room).toBe('Lobby');
  expect(state.people).toEqual([
    { name: 'Alice', avatar_url: null, speaking: false, muted: false },
    { name: 'anna', avatar_url: 'https://example.com/a.png', speaking: true, muted: true },
  ]);
});

it('my own speaking and muted state come from the microphone, not from the server list', () => {
  const state = buildOverlayState(input({ localSpeaking: true, localMuted: true, participants: [participant({ is_muted: false })] }));
  expect(state.people[0]).toMatchObject({ speaking: true, muted: true });
  expect(state.me_muted).toBe(true);
});

it('outside a room the state has no room and no people', () => {
  expect(buildOverlayState(input({ room: null }))).toEqual({ room: null, people: [], me_muted: false });
});

it('the game rule shows the window when my game starts and hides it only when it had shown it', () => {
  const base = { enabled: true, autoInGame: true, inGame: true, visible: false, shownAutomatically: false };
  expect(autoOverlayAction(base)).toBe('show');
  expect(autoOverlayAction({ ...base, autoInGame: false })).toBe('none');
  expect(autoOverlayAction({ ...base, visible: true })).toBe('none');
  expect(autoOverlayAction({ enabled: true, autoInGame: true, inGame: false, visible: true, shownAutomatically: true })).toBe('hide');
  expect(autoOverlayAction({ enabled: true, autoInGame: true, inGame: false, visible: true, shownAutomatically: false })).toBe('none');
});

it('applies the game rule to my game and leaves a window that I opened myself alone', async () => {
  setPrefs({ ...DEFAULT_PREFS, overlay: { ...DEFAULT_PREFS.overlay, autoInGame: true } });
  setGameActivity({ user_id: ME, game: 'Dota 2' });
  expect(await applyAutoOverlay()).toBe('show');
  expect(invoke).toHaveBeenCalledWith('overlay_show');
  expect(overlayOpen()).toBe(true);

  setGameActivity({ user_id: ME, game: null });
  expect(await applyAutoOverlay()).toBe('hide');
  expect(invoke).toHaveBeenCalledWith('overlay_hide');
  expect(overlayOpen()).toBe(false);

  // Opened by hand: a game closing must not close it.
  setOverlayOpen(true);
  expect(await applyAutoOverlay()).toBe('none');
  expect(overlayOpen()).toBe(true);
});

it('another person playing does not show the window for me', async () => {
  setPrefs({ ...DEFAULT_PREFS, overlay: { ...DEFAULT_PREFS.overlay, autoInGame: true } });
  setGameActivity({ user_id: ANNA, game: 'Chess' });
  expect(await applyAutoOverlay()).toBe('none');
  expect(invoke).not.toHaveBeenCalledWith('overlay_show');
});

it('the overlay prefs are repaired when read: bad values fall back, the opacity stays in range', () => {
  expect(parsePrefs(null).overlay).toEqual({ enabled: true, opacity: 0.85, autoInGame: false, clickThrough: false });
  const repaired = parsePrefs(JSON.stringify({ overlay: { enabled: 0, opacity: 7, autoInGame: 'yes', clickThrough: true } }));
  expect(repaired.overlay).toEqual({ enabled: true, opacity: 1, autoInGame: false, clickThrough: true });
  expect(parsePrefs(JSON.stringify({ overlay: { opacity: 0.01 } })).overlay.opacity).toBe(0.3);
  expect(parsePrefs(JSON.stringify({ overlay: 'broken' })).overlay.opacity).toBe(0.85);
  expect(prefs().overlay.opacity).toBe(0.85);
});

it('turned off, the game rule never shows the mini window', () => {
  expect(autoOverlayAction({ enabled: false, autoInGame: true, inGame: true, visible: false, shownAutomatically: false })).toBe('none');
});

it('turned off, showing it does nothing and the backend is not asked to show it', async () => {
  setPrefs({ ...DEFAULT_PREFS, overlay: { ...DEFAULT_PREFS.overlay, enabled: false } });
  await showOverlay();
  expect(invoke).not.toHaveBeenCalledWith('overlay_show');
  expect(overlayOpen()).toBe(false);
});

it('turning it off hides an open window and tells the backend', async () => {
  setPrefs({ ...DEFAULT_PREFS, overlay: { ...DEFAULT_PREFS.overlay, enabled: true } });
  setOverlayOpen(true);
  setOverlayPref({ enabled: false });
  await new Promise((resolve) => setTimeout(resolve, 0));
  expect(invoke).toHaveBeenCalledWith('overlay_set_enabled', { enabled: false });
  expect(invoke).toHaveBeenCalledWith('overlay_hide');
  expect(overlayOpen()).toBe(false);
  expect(prefs().overlay.enabled).toBe(false);
});
