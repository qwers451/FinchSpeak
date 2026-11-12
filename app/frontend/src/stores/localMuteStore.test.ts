import { beforeEach, expect, it, vi } from 'vitest';

const userId = '9c12a901-103d-43e1-95b3-8fe4e9962ec2';

beforeEach(() => {
  vi.restoreAllMocks();
  vi.resetModules();
  localStorage.clear();
});

it('restores valid user IDs from storage and ignores invalid entries', async () => {
  localStorage.setItem('finchspeak.localMutes', JSON.stringify([userId, userId, 'bad-id', 42]));
  const { isLocallyMuted } = await import('./localMuteStore');
  expect(isLocallyMuted(userId)).toBe(true);
  expect(isLocallyMuted('bad-id')).toBe(false);
});

it('keeps voice muted when the user returns with a different peer ID', async () => {
  const { api } = await import('../services/api');
  const mixer = vi.spyOn(api, 'setSpeakerVolume').mockResolvedValue();
  const { toggleLocalMute } = await import('./localMuteStore');
  const { applySavedVolume, screenAudioKey, setVolumes } = await import('./volumeStore');
  setVolumes({ [userId]: 0.65, [`screen:${userId}`]: 1.5 });
  await toggleLocalMute(userId, 40);
  expect(JSON.parse(localStorage.getItem('finchspeak.localMutes') ?? 'null')).toEqual([userId]);
  await applySavedVolume(userId, 41);
  expect(mixer.mock.calls).toEqual([[40, 0], [41, 0], [screenAudioKey(41), 1.5]]);
});

it('restores saved voice gain for every connected device when unmuted', async () => {
  const { api } = await import('../services/api');
  const mixer = vi.spyOn(api, 'setSpeakerVolume').mockResolvedValue();
  const { setVoiceParticipants } = await import('./voiceStore');
  const { toggleLocalMute } = await import('./localMuteStore');
  const { setVolumes, volumeFor } = await import('./volumeStore');
  const participant = {
    channel_id: 'room', user_id: userId, username: 'User', is_muted: false, is_deafened: false, is_sharing: false,
  };
  setVoiceParticipants([{ ...participant, peer_id: 40 }, { ...participant, peer_id: 41 }]);
  setVolumes({ [userId]: 0.65 });
  await toggleLocalMute(userId, 40);
  expect(volumeFor(userId)).toBe(0.65);
  await toggleLocalMute(userId, 41);
  expect(mixer.mock.calls).toEqual([[40, 0], [41, 0], [41, 0.65], [40, 0.65]]);
  expect(JSON.parse(localStorage.getItem('finchspeak.localMutes') ?? 'null')).toEqual([]);
});
