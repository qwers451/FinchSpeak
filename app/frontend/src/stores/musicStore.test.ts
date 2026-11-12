import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { api } from '../services/api';
import { describeError } from '../i18n/errors';
import type { Channel, MusicTrack } from '../types';
import { setActiveVoiceChannel } from './voiceStore';
import { setServerFeatures } from './sessionStore';
import { appError, setAppError } from './notificationStore';
import {
  applyMusicError, applyMusicState, musicAvailable, musicPosition, musicState, musicVolume, resetMusic,
  setMusicVolume, toggleMusicMute,
} from './musicStore';
import { DEFAULT_MUSIC_VOLUME } from './volumeStore';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));

const ROOM = '2a7c1e3e-6f0b-4f55-9d55-7d1f0c1a8b10';
const OTHER_ROOM = '5b1f0c2d-1a2b-4c3d-8e9f-0a1b2c3d4e5f';
const USER = '9c12a901-103d-43e1-95b3-8fe4e9962ec2';
const TRACK_ID = '7e8f9a0b-1c2d-4e3f-8a9b-0c1d2e3f4a5b';

const track: MusicTrack = {
  id: TRACK_ID, uploader_id: USER, title: 'Tune', artist: null, duration_ms: 180_000,
  size_bytes: 1_000, format: 'mp3', temporary: false, created_at: 1,
};
const entry = { entry_id: '0f1e2d3c-4b5a-4968-8778-6758493a2b1c', track, added_by: USER };
const room = (id: string) => ({ id } as unknown as Channel);

function playing(overrides: { paused?: boolean; position_ms?: number; at_server_ms?: number } = {}) {
  return { entry, position_ms: 10_000, at_server_ms: 49_000, paused: false, ...overrides };
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(1_000_000);
  localStorage.clear();
  resetMusic();
  setActiveVoiceChannel(room(ROOM));
  setServerFeatures(['music']);
  setAppError(null);
  vi.mocked(invoke).mockReset();
});

afterEach(() => {
  resetMusic();
  setActiveVoiceChannel(null);
  vi.useRealTimers();
  vi.restoreAllMocks();
  delete (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
});

describe('room music state', () => {
  it('moves the position on while playing and keeps it while paused', () => {
    applyMusicState({ channel_id: ROOM, server_ms: 50_000, state: { queue: [entry], playing: playing() } });
    // At the moment of the message the track is 1 s past its anchor: 10 s + (50 s − 49 s).
    expect(musicPosition()).toBe(11_000);
    vi.advanceTimersByTime(2_000);
    expect(musicPosition()).toBe(13_000);

    applyMusicState({ channel_id: ROOM, server_ms: 52_000, state: { queue: [entry], playing: playing({ paused: true }) } });
    vi.advanceTimersByTime(5_000);
    expect(musicPosition()).toBe(10_000);
  });

  it('shows nothing once I am out of that room', () => {
    applyMusicState({ channel_id: ROOM, server_ms: 50_000, state: { queue: [entry], playing: playing() } });
    expect(musicState()?.queue).toHaveLength(1);
    setActiveVoiceChannel(null);
    expect(musicState()).toBeNull();
    expect(musicPosition()).toBe(0);
  });

  it('a state for another room replaces the old one', () => {
    applyMusicState({ channel_id: ROOM, server_ms: 50_000, state: { queue: [entry], playing: playing() } });
    setActiveVoiceChannel(room(OTHER_ROOM));
    applyMusicState({ channel_id: OTHER_ROOM, server_ms: 50_000, state: { queue: [], playing: null } });
    expect(musicState()).toEqual({ queue: [], playing: null });
    expect(musicPosition()).toBe(0);
  });

  it('music-error shows the translated text as a toast', () => {
    applyMusicError({ message: 'Music queue is full' });
    expect(appError()).toBe(describeError('Music queue is full'));
  });

  it('is available only when the server announces music', () => {
    expect(musicAvailable()).toBe(true);
    setServerFeatures(['activity']);
    expect(musicAvailable()).toBe(false);
  });
});

describe('room music volume', () => {
  it('defaults to half and remembers the value under the music key', async () => {
    expect(musicVolume()).toBe(DEFAULT_MUSIC_VOLUME);
    const sent = vi.spyOn(api, 'musicSetVolume').mockResolvedValue();
    await setMusicVolume(0.3);
    expect(musicVolume()).toBe(0.3);
    expect(sent).toHaveBeenCalledWith(0.3);
    expect(JSON.parse(localStorage.getItem('finchspeak.volumes') ?? '{}')).toMatchObject({ music: 0.3 });
  });

  it('clamps the volume to 0..1', async () => {
    vi.spyOn(api, 'musicSetVolume').mockResolvedValue();
    await setMusicVolume(4);
    expect(musicVolume()).toBe(1);
  });

  it('mute remembers the previous volume and a second press restores it', async () => {
    const sent = vi.spyOn(api, 'musicSetVolume').mockResolvedValue();
    await setMusicVolume(0.8);
    await toggleMusicMute();
    expect(musicVolume()).toBe(0);
    expect(sent).toHaveBeenLastCalledWith(0);
    await toggleMusicMute();
    expect(musicVolume()).toBe(0.8);
    expect(sent).toHaveBeenLastCalledWith(0.8);
  });
});

describe('music upload command', () => {
  it('sends the file as raw bytes with the name and keep flag in headers', async () => {
    (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    vi.mocked(invoke).mockResolvedValue(track);
    const file = new File([new Uint8Array([1, 2, 3])], 'Песня.mp3', { type: 'audio/mpeg' });

    await expect(api.musicUpload(file, true)).resolves.toEqual(track);

    const [command, payload, options] = vi.mocked(invoke).mock.calls[0] ?? [];
    expect(command).toBe('music_upload');
    expect(Array.isArray(payload)).toBe(false);
    expect(payload).toBeInstanceOf(ArrayBuffer);
    expect(options).toEqual({ headers: { 'x-finch-file-name': encodeURIComponent('Песня.mp3'), 'x-finch-keep': 'true' } });
  });
});
