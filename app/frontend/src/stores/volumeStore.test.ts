import { beforeEach, expect, it, vi } from 'vitest';
import { api } from '../services/api';
import { applySavedVolume, DEFAULT_VOLUME, screenAudioKey, setVolume, setVolumes, volumeFor, volumes } from './volumeStore';

beforeEach(() => {
  vi.restoreAllMocks();
  localStorage.clear();
  setVolumes({});
});

it('is 100 % for someone never adjusted', () => {
  expect(volumeFor('u1')).toBe(DEFAULT_VOLUME);
});

it('sets the volume in the mixer and remembers it for the next call', async () => {
  const mixer = vi.spyOn(api, 'setSpeakerVolume').mockResolvedValue();
  await setVolume('u1', 42, 1.5);
  expect(mixer).toHaveBeenCalledWith(42, 1.5);
  expect(volumeFor('u1')).toBe(1.5);
  expect(JSON.parse(localStorage.getItem('finchspeak.volumes')!)).toEqual({ u1: 1.5 });
});

it('keeps volumes inside 0..200 % and does not store the default', async () => {
  vi.spyOn(api, 'setSpeakerVolume').mockResolvedValue();
  await setVolume('u1', 1, 7);
  await setVolume('u2', 2, -3);
  await setVolume('u3', 3, 1);
  expect(volumes()).toEqual({ u1: 2, u2: 0, u3: 1 });
  expect(JSON.parse(localStorage.getItem('finchspeak.volumes')!)).toEqual({ u1: 2, u2: 0 });
});

it('applies the remembered voice and screen volumes when the person (new peer id) appears', async () => {
  const mixer = vi.spyOn(api, 'setSpeakerVolume').mockResolvedValue();
  setVolumes({ u1: 0.5, 'screen:u1': 1.5, u2: 1 });
  await applySavedVolume('u1', 77);
  await applySavedVolume('u2', 78);
  await applySavedVolume('u3', 79);
  expect(mixer.mock.calls).toEqual([[77, 0.5], [screenAudioKey(77), 1.5]]);
});

it('survives broken storage and a mixer that refuses', async () => {
  localStorage.setItem('finchspeak.volumes', '{not json');
  expect(volumeFor('u1')).toBe(1);
  vi.spyOn(api, 'setSpeakerVolume').mockRejectedValue(new Error('no audio'));
  setVolumes({ u1: 0.5 });
  await expect(applySavedVolume('u1', 1)).resolves.toBeUndefined();
});
