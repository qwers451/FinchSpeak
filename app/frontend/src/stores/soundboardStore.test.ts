import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { api } from '../services/api';
import type { SoundInfo, User } from '../types';
import { setCurrentUser } from './authStore';
import { toggleLocalMute } from './localMuteStore';
import { boardSounds, canRemove, handleSoundPlayed, volumeGain, loadBoard, removeBoardSound, resetSoundboardForTests, setSoundboardHidden, soundboardHidden, uploadBoardSound } from './soundboardStore';
import { setIsDeafened } from './voiceStore';
import { DEFAULT_SOUNDBOARD_VOLUME, setVolume, volumeFor } from './volumeStore';

// The audio graph is recorded instead of played.
const starts: unknown[] = [];
const gains: { gain: { value: number } }[] = [];
const fakeContext = {
  destination: {},
  decodeAudioData: vi.fn(async (bytes: ArrayBuffer) => ({ duration: 1, byteLength: bytes.byteLength })),
  createBufferSource: () => {
    const source = { buffer: null as unknown, connect: () => source, start: () => starts.push(source.buffer) };
    return source;
  },
  createGain: () => {
    const gain = { gain: { value: 1 }, connect: () => gain };
    gains.push(gain);
    return gain;
  },
};
vi.mock('./tones', () => ({ audioContext: () => fakeContext, playTone: vi.fn() }));
vi.mock('./joinSoundEncode', () => ({ encodeBoardSound: vi.fn(async () => new Uint8Array([1, 2, 3])) }));

const flush = () => new Promise((resolve) => setTimeout(resolve, 0));
const sound = (id: string, uploader = 'bob', name = 'Гонг'): SoundInfo => ({ id, name, emoji: null, uploader_id: uploader, duration_ms: 1000 });

beforeEach(() => {
  resetSoundboardForTests();
  starts.length = 0;
  gains.length = 0;
  setIsDeafened(false);
  setSoundboardHidden(false);
  void setVolume('soundboard', undefined, DEFAULT_SOUNDBOARD_VOLUME);
  vi.spyOn(api, 'downloadBoardSound').mockResolvedValue(new ArrayBuffer(8));
  vi.spyOn(api, 'setSpeakerVolume').mockResolvedValue();
});

afterEach(() => {
  vi.restoreAllMocks();
  localStorage.clear();
});

it('plays a played sound from the cache at my own soundboard volume', async () => {
  handleSoundPlayed({ sound_id: 'gong', user_id: 'bob' });
  handleSoundPlayed({ sound_id: 'gong', user_id: 'carol' });
  await flush();
  expect(api.downloadBoardSound).toHaveBeenCalledTimes(1);
  expect(starts).toHaveLength(2);
  expect(gains.at(-1)?.gain.value).toBeCloseTo(0.64, 6);
});

it('the soundboard defaults to 0.8 and remembers my change', async () => {
  expect(volumeFor('soundboard')).toBe(0.8);
  await setVolume('soundboard', undefined, 1.5);
  expect(volumeFor('soundboard')).toBe(1.5);
  handleSoundPlayed({ sound_id: 'gong', user_id: 'bob' });
  await flush();
  expect(gains.at(-1)?.gain.value).toBe(1.5);
});

it("uses the perceptual curve: squared up to 100 %, linear to 200 %", () => {
  expect(volumeGain(1)).toBe(1);
  expect(volumeGain(0.5)).toBe(0.25);
  expect(volumeGain(0)).toBe(0);
  expect(volumeGain(1.5)).toBe(1.5);
  expect(volumeGain(2)).toBe(2);
  expect(volumeGain(3)).toBe(2);
  expect(volumeGain(Number.NaN)).toBe(0);
});

it('does not play for a person I muted locally', async () => {
  await toggleLocalMute('bob');
  handleSoundPlayed({ sound_id: 'gong', user_id: 'bob' });
  await flush();
  expect(starts).toHaveLength(0);
  await toggleLocalMute('bob');
});

it('does not play while I am deafened, or when I chose not to hear the board', async () => {
  setIsDeafened(true);
  handleSoundPlayed({ sound_id: 'gong', user_id: 'bob' });
  setIsDeafened(false);
  setSoundboardHidden(true);
  expect(localStorage.getItem('finchspeak.soundboardHidden')).toBe('true');
  handleSoundPlayed({ sound_id: 'gong', user_id: 'bob' });
  await flush();
  expect(starts).toHaveLength(0);
  setSoundboardHidden(false);
  expect(soundboardHidden()).toBe(false);
});

it('plays nothing at zero volume', async () => {
  await setVolume('soundboard', undefined, 0);
  handleSoundPlayed({ sound_id: 'gong', user_id: 'bob' });
  await flush();
  expect(starts).toHaveLength(0);
});

it('loads, adds and removes sounds through the API', async () => {
  vi.spyOn(api, 'soundboard').mockResolvedValue([sound('gong')]);
  vi.spyOn(api, 'uploadBoardSound').mockResolvedValue(sound('horn', 'alice', 'Рог'));
  vi.spyOn(api, 'deleteBoardSound').mockResolvedValue();
  await loadBoard();
  expect(boardSounds().map((item) => item.id)).toEqual(['gong']);

  await uploadBoardSound(new File([], 'horn.ogg'), 'Рог', '📯');
  expect(api.uploadBoardSound).toHaveBeenCalledWith(new Uint8Array([1, 2, 3]), 'Рог', '📯');
  expect(boardSounds().map((item) => item.id)).toEqual(['gong', 'horn']);

  await removeBoardSound('gong');
  expect(api.deleteBoardSound).toHaveBeenCalledWith('gong');
  expect(boardSounds().map((item) => item.id)).toEqual(['horn']);
});

it('the uploader and moderators may remove a sound; other members may not', () => {
  setCurrentUser({ id: 'alice', username: 'alice', display_name: 'Alice', role: 'member' } as unknown as User);
  expect(canRemove(sound('a', 'alice'))).toBe(true);
  expect(canRemove(sound('b', 'bob'))).toBe(false);
  setCurrentUser({ id: 'alice', username: 'alice', display_name: 'Alice', role: 'moderator' } as unknown as User);
  expect(canRemove(sound('b', 'bob'))).toBe(true);
});
