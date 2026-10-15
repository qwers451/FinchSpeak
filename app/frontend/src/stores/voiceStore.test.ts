import { beforeEach, expect, it, vi } from 'vitest';
import { api } from '../services/api';
import { setActiveVoiceChannel, setVoiceParticipants, voiceParticipants, toggleDeafen, isMuted, setIsMuted, setIsDeafened } from './voiceStore';

beforeEach(() => {
  vi.restoreAllMocks();
  setActiveVoiceChannel({ id: 'ours', short_id: 1, name: 'ours', channel_type: 'voice', position: 0, bitrate_kbps: 64 });
  setVoiceParticipants([]);
  setIsMuted(false);
  setIsDeafened(false);
});

it('AUD-020 shows participants only from the active voice channel', () => {
  setVoiceParticipants([
    { user_id: 'a', peer_id: 1, username: 'A', channel_id: 'ours' },
    { user_id: 'b', peer_id: 2, username: 'B', channel_id: 'theirs' },
  ]);
  expect(voiceParticipants().map((participant) => participant.user_id)).toEqual(['a']);
});

it.each([false, true])('AUD-033 restores previous mute=%s after deafen', async (muted) => {
  vi.spyOn(api, 'setVoiceState').mockResolvedValue();
  setIsMuted(muted);
  await toggleDeafen();
  expect(isMuted()).toBe(true);
  await toggleDeafen();
  expect(isMuted()).toBe(muted);
});
