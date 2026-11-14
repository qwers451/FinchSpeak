import { render } from 'solid-js/web';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(async () => undefined), Channel: class { onmessage: unknown = null; } }));

import { api } from '../../services/api';
import { t } from '../../i18n';
import { setActiveChannel, setChannels } from '../../stores/chatStore';
import { applyMusicState, musicPanelOpen, resetMusic, setMusicPanelOpen } from '../../stores/musicStore';
import { resetSessionState, setServerFeatures } from '../../stores/sessionStore';
import { setSoundboardOpen, soundboardOpen } from '../../stores/soundboardStore';
import { setActiveVoiceChannel } from '../../stores/voiceStore';
import { DEFAULT_PREFS, setPrefs } from '../../stores/settingsStore';
import type { Channel, MusicRoomState, MusicTrack } from '../../types';
import { RoomToolWindows, RoomTools } from './RoomTools';

const room = { id: 'room', name: 'Lounge', channel_type: 'voice' } as unknown as Channel;
const text = { id: 'text', name: 'general', channel_type: 'text' } as unknown as Channel;
const track: MusicTrack = { id: 't1', uploader_id: 'bob', title: 'Blue Hour', artist: null, duration_ms: 200_000, size_bytes: 1, format: 'mp3', temporary: false, created_at: 0 };
const playing = (paused = false): MusicRoomState => ({
  queue: [],
  playing: { entry: { entry_id: 'e1', track, added_by: 'bob' }, paused, position_ms: 5_000, at_server_ms: 0 },
});

let dispose: (() => void) | undefined;
let host: HTMLElement;
const mount = (view: () => unknown) => {
  host = document.createElement('div');
  document.body.appendChild(host);
  dispose = render(view as () => never, host);
};
const flush = () => new Promise((resolve) => setTimeout(resolve, 0));
const button = (name: string) => Array.from(document.querySelectorAll<HTMLButtonElement>('button')).find((b) => (b.textContent ?? '').trim() === name || b.getAttribute('aria-label') === name);

beforeEach(() => {
  vi.restoreAllMocks();
  setPrefs({ ...DEFAULT_PREFS, appearance: { ...DEFAULT_PREFS.appearance, language: 'en' } });
  resetSessionState();
  vi.spyOn(api, 'musicCommand').mockResolvedValue();
  vi.spyOn(api, 'musicSetVolume').mockResolvedValue();
  vi.spyOn(api, 'musicLibrary').mockResolvedValue([]);
});

afterEach(() => {
  dispose?.();
  dispose = undefined;
  host?.remove();
  document.body.innerHTML = '';
  resetMusic();
  setActiveVoiceChannel(null);
  setActiveChannel(null);
  setChannels([]);
  setServerFeatures([]);
  setMusicPanelOpen(false);
  setSoundboardOpen(false);
});

it('is absent outside a voice room', () => {
  setServerFeatures(['music', 'soundboard']);
  mount(() => <RoomTools />);
  expect(document.querySelector('.room-tools')).toBeNull();
});

it('in a room it offers Music, Sounds (only with the soundboard feature) and Screen', () => {
  setActiveVoiceChannel(room);
  setServerFeatures(['music']);
  mount(() => <RoomTools />);
  expect(button(t('Music'))).toBeTruthy();
  expect(button(t('Screen'))).toBeTruthy();
  expect(button(t('Sounds'))).toBeUndefined();
  setServerFeatures(['music', 'soundboard']);
  expect(button(t('Sounds'))).toBeTruthy();
});

it('shows the track and its clock only while something plays', () => {
  setActiveVoiceChannel(room);
  setServerFeatures(['music']);
  mount(() => <RoomTools />);
  expect(document.querySelector('.room-now')).toBeNull();
  applyMusicState({ channel_id: 'room', state: playing(), server_ms: 0 });
  const row = document.querySelector('.room-now')!;
  expect(row.textContent).toContain('Blue Hour');
  expect(row.querySelector('.music-clock')?.textContent).toMatch(/^\d+:\d\d$/);
  expect(row.querySelector('.trunc')?.getAttribute('title')).toBe('Blue Hour');
});

it('the pause button sends a pause command, and resume when paused', async () => {
  setActiveVoiceChannel(room);
  setServerFeatures(['music']);
  mount(() => <RoomTools />);
  applyMusicState({ channel_id: 'room', state: playing(), server_ms: 0 });
  button(t('Pause'))!.click();
  await flush();
  expect(api.musicCommand).toHaveBeenCalledWith({ kind: 'pause' });
  applyMusicState({ channel_id: 'room', state: playing(true), server_ms: 0 });
  button(t('Resume'))!.click();
  await flush();
  expect(api.musicCommand).toHaveBeenCalledWith({ kind: 'resume' });
  button(t('Next'))!.click();
  expect(api.musicCommand).toHaveBeenCalledWith({ kind: 'skip' });
});

it('Music opens the player window from a text channel; the windows are mounted outside the room view', async () => {
  setActiveVoiceChannel(room);
  setActiveChannel(text);
  setServerFeatures(['music', 'soundboard']);
  mount(() => <><RoomTools /><RoomToolWindows /></>);
  expect(document.querySelector('.music-panel')).toBeNull();
  button(t('Music'))!.click();
  await flush();
  expect(musicPanelOpen()).toBe(true);
  expect(document.querySelector('.modal .music-panel')).not.toBeNull();
  button(t('Sounds'))!.click();
  expect(soundboardOpen()).toBe(true);
  await flush();
  expect(document.querySelector('.soundboard-panel')).not.toBeNull();
});

it('leaving the room closes both windows and clears the flags', async () => {
  setActiveVoiceChannel(room);
  setServerFeatures(['music', 'soundboard']);
  mount(() => <RoomToolWindows />);
  setMusicPanelOpen(true);
  setSoundboardOpen(true);
  await flush();
  setActiveVoiceChannel(null);
  expect(musicPanelOpen()).toBe(false);
  expect(soundboardOpen()).toBe(false);
});

it('the Screen button is not pressed when nothing is shared', () => {
  setActiveVoiceChannel(room);
  mount(() => <RoomTools />);
  expect(button(t('Screen'))?.getAttribute('aria-pressed')).toBe('false');
});
