import { beforeEach, expect, it, vi } from 'vitest';
import { api } from '../services/api';
import { endedShare, closeEndedShare, watchShare, watching } from './screenStore';
import { resetSessionState, sessionHandlers } from './sessionStore';
import { setAppError, appError } from './notificationStore';
import { setVolumes } from './volumeStore';
import {
  levelFraction, MIC_FLOOR_DB, micLevelDb, micTestPhase, setVoiceParticipants, startMicTest, voiceQuality,
} from './voiceStore';

beforeEach(() => {
  vi.restoreAllMocks();
  resetSessionState();
  setAppError(null);
  setVolumes({});
  localStorage.clear();
  setVoiceParticipants([
    { user_id: 'alice', peer_id: 20, username: 'Alice', channel_id: 'room' },
    { user_id: 'bob', peer_id: 21, username: 'Bob', channel_id: 'room' },
  ]);
});

const watchAlice = async () => {
  vi.spyOn(api, 'subscribeScreenShare').mockResolvedValue();
  await watchShare('alice', 20);
};

it('maps a level in dB onto the meter', () => {
  expect(levelFraction(MIC_FLOOR_DB)).toBe(0);
  expect(levelFraction(-120)).toBe(0);
  expect(levelFraction(0)).toBe(1);
  expect(levelFraction(-35)).toBeCloseTo(0.5);
});

it('keeps the live level, the test phase and link quality from backend events', () => {
  sessionHandlers['mic-level']({ db: -23.5, speaking: true });
  expect(micLevelDb()).toBe(-23.5);
  sessionHandlers['mic-level']({ db: Number.NaN, speaking: false });
  expect(micLevelDb()).toBe(MIC_FLOOR_DB);

  sessionHandlers['mic-test']({ phase: 'recording' });
  expect(micTestPhase()).toBe('recording');
  sessionHandlers['mic-test']({ phase: 'done' });
  expect(micTestPhase()).toBe('idle');

  sessionHandlers['voice-quality']([{ peer_id: 21, quality: 'poor', loss: 0.12 }]);
  expect(voiceQuality()).toEqual({ 21: { quality: 'poor', loss: 0.12 } });
  sessionHandlers['voice-quality']([]);
  expect(voiceQuality()).toEqual({});
});

it('a refused microphone test is explained, not thrown', async () => {
  vi.spyOn(api, 'testMicrophone').mockRejectedValue(new Error('Join a voice channel first'));
  await startMicTest();
  expect(appError()).toBe('Join a voice channel first');
});

it('a watched share that stops leaves an explanation behind instead of vanishing', async () => {
  await watchAlice();
  sessionHandlers['screen-share-stopped']({ peer_id: 20, reason: 'Stopped' });
  expect(watching()).toBeNull();
  expect(endedShare()).toEqual({ user_id: 'alice', peer_id: 20, name: 'Alice', reason: 'stopped' });
  closeEndedShare();
  expect(endedShare()).toBeNull();
});

it('says "left" when the sharer left the room and "disconnected" when the connection dropped', async () => {
  await watchAlice();
  sessionHandlers['screen-share-stopped']({ peer_id: 20, reason: 'Left' });
  expect(endedShare()?.reason).toBe('left');
  closeEndedShare();

  await watchAlice();
  sessionHandlers['screen-share-stopped']({ peer_id: 20, reason: 'Disconnected' });
  expect(endedShare()?.reason).toBe('disconnected');
});

it('also explains it when the first word is the sharer leaving the room', async () => {
  await watchAlice();
  sessionHandlers['user-left-voice']({ user_id: 'alice', peer_id: 20, channel_id: 'room' });
  expect(endedShare()).toMatchObject({ name: 'Alice', reason: 'left' });
});

it('stopping shares nobody watches leaves no screen behind, and watching another clears the old one', async () => {
  sessionHandlers['screen-share-stopped']({ peer_id: 99, reason: 'Stopped' });
  expect(endedShare()).toBeNull();
  await watchAlice();
  sessionHandlers['screen-share-stopped']({ peer_id: 20 });
  expect(endedShare()).not.toBeNull();
  vi.spyOn(api, 'subscribeScreenShare').mockResolvedValue();
  await watchShare('bob', 21);
  expect(endedShare()).toBeNull();
});

it('a person who joins the room gets their remembered volume', () => {
  const mixer = vi.spyOn(api, 'setSpeakerVolume').mockResolvedValue();
  setVolumes({ carol: 0.4 });
  sessionHandlers['user-joined-voice']({ user_id: 'carol', peer_id: 30, username: 'Carol', channel_id: 'room' });
  expect(mixer).toHaveBeenCalledWith(30, 0.4);
});
