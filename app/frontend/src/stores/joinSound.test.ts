import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { api } from '../services/api';
import { encodeWav } from './joinSoundEncode';
import { clearJoinSounds, invalidateJoinSound, playJoinSound } from './joinSound';
import { DEFAULT_PREFS, setPrefs } from './settingsStore';

const frequencies: number[] = [];
const started: number[] = [];
vi.stubGlobal('AudioContext', class {
  currentTime = 0;
  destination = {};
  createOscillator() {
    return { type: '', frequency: { set value(hz: number) { frequencies.push(hz); } }, connect: () => ({ connect: () => {} }), start() {}, stop() {} };
  }
  createGain() {
    return { gain: { value: 0, setValueAtTime() {}, linearRampToValueAtTime() {}, exponentialRampToValueAtTime() {} }, connect: () => ({ connect: () => {} }) };
  }
  createBufferSource() {
    return { buffer: null, connect: () => ({ connect: () => {} }), start: () => { started.push(1); } };
  }
  decodeAudioData = async () => ({ duration: 0.5 });
});

beforeEach(() => {
  frequencies.length = 0;
  started.length = 0;
  clearJoinSounds();
  setPrefs(DEFAULT_PREFS);
});
afterEach(() => vi.restoreAllMocks());

it('encodes a 16-bit mono PCM WAV header and samples', () => {
  const wav = encodeWav(new Float32Array([0, 1, -1, 2]), 24000);
  const view = new DataView(wav.buffer);
  expect(String.fromCharCode(...wav.slice(0, 4))).toBe('RIFF');
  expect(String.fromCharCode(...wav.slice(8, 16))).toBe('WAVEfmt ');
  expect(view.getUint16(20, true)).toBe(1);
  expect(view.getUint16(22, true)).toBe(1);
  expect(view.getUint32(24, true)).toBe(24000);
  expect(view.getUint16(34, true)).toBe(16);
  expect(view.getUint32(40, true)).toBe(8);
  expect(wav.length).toBe(44 + 8);
  expect([view.getInt16(44, true), view.getInt16(46, true), view.getInt16(48, true), view.getInt16(50, true)]).toEqual([0, 32767, -32768, 32767]);
});

it('falls back to the default join tone when the person has no sound', async () => {
  vi.spyOn(api, 'downloadJoinSound').mockResolvedValue(null);
  await playJoinSound('u1');
  expect(frequencies).toEqual([660, 880]);
  expect(started).toEqual([]);
});

it('plays the custom sound once decoded, and again after it is invalidated', async () => {
  const download = vi.spyOn(api, 'downloadJoinSound').mockResolvedValue(new ArrayBuffer(8));
  await playJoinSound('u1');
  await playJoinSound('u1');
  expect(started).toHaveLength(2);
  expect(download).toHaveBeenCalledTimes(1);
  expect(frequencies).toEqual([]);
  invalidateJoinSound('u1');
  await playJoinSound('u1');
  expect(download).toHaveBeenCalledTimes(2);
});

it('stays silent when sounds are off', async () => {
  vi.spyOn(api, 'downloadJoinSound').mockResolvedValue(null);
  setPrefs({ ...DEFAULT_PREFS, notifications: { ...DEFAULT_PREFS.notifications, sounds: false } });
  await playJoinSound('u1');
  expect(frequencies).toEqual([]);
});
