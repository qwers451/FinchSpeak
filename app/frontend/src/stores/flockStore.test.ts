import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { api } from '../services/api';
import { setChannels } from './chatStore';
import { activeVoiceChannel, setActiveVoiceChannel } from './voiceStore';
import { acceptCall, callFlock, callUserHere, dismissCall, FLOCK_CALL_TTL_MS, handleFlockRefusal, pendingCalls, receiveFlockCall } from './flockStore';
import { sessionHandlers, setKnownUsers } from './sessionStore';
import { appError } from './notificationStore';
import { t } from '../i18n';

const home = { id: 'home', short_id: 1, name: 'Home', channel_type: 'voice' as const, position: 0, bitrate_kbps: 64 };
const lounge = { id: 'lounge', short_id: 2, name: 'Lounge', channel_type: 'voice' as const, position: 1, bitrate_kbps: 64 };
const call = { from_user_id: 'alice', from_name: 'Alice', channel_id: 'lounge' };

beforeEach(() => {
  vi.useFakeTimers();
  setChannels([home, lounge]);
  setActiveVoiceChannel(home);
  vi.spyOn(api, 'joinVoiceChannel').mockResolvedValue();
  vi.spyOn(api, 'callFlock').mockResolvedValue();
  vi.spyOn(api, 'callUser').mockResolvedValue();
});

afterEach(() => {
  for (const pending of pendingCalls()) dismissCall(pending.id);
  vi.useRealTimers();
  vi.restoreAllMocks();
});

it('a call waits for an answer and closes by itself after 30 seconds', () => {
  receiveFlockCall(call);
  expect(pendingCalls()).toHaveLength(1);
  vi.advanceTimersByTime(FLOCK_CALL_TTL_MS - 1);
  expect(pendingCalls()).toHaveLength(1);
  vi.advanceTimersByTime(1);
  expect(pendingCalls()).toHaveLength(0);
});

it('a new call from the same person replaces their card', () => {
  receiveFlockCall(call);
  receiveFlockCall({ ...call, channel_id: 'home' });
  expect(pendingCalls().map((pending) => pending.channel_id)).toEqual(['home']);
});

it('the session event opens a card', () => {
  sessionHandlers['flock-call'](call);
  expect(pendingCalls()).toHaveLength(1);
});

it('Fly joins the room it was called to', async () => {
  receiveFlockCall(call);
  await acceptCall(pendingCalls()[0].id);
  expect(api.joinVoiceChannel).toHaveBeenCalledWith('lounge', 2);
  expect(pendingCalls()).toHaveLength(0);
  expect(activeVoiceChannel()?.id).toBe('lounge');
});

it('Fly does nothing when I am already in that room', async () => {
  setActiveVoiceChannel(lounge);
  receiveFlockCall(call);
  await acceptCall(pendingCalls()[0].id);
  expect(api.joinVoiceChannel).not.toHaveBeenCalled();
  expect(pendingCalls()).toHaveLength(0);
});

it('Stay only closes the card', () => {
  receiveFlockCall(call);
  dismissCall(pendingCalls()[0].id);
  expect(pendingCalls()).toHaveLength(0);
  expect(api.joinVoiceChannel).not.toHaveBeenCalled();
});

it('calling my room to another room moves me there too', async () => {
  await callFlock(lounge);
  expect(api.callFlock).toHaveBeenCalledWith('lounge');
  expect(api.joinVoiceChannel).toHaveBeenCalledWith('lounge', 2);
  expect(activeVoiceChannel()?.id).toBe('lounge');
});

it('calling one person sends the call without moving me', async () => {
  await callUserHere('bob');
  expect(api.callUser).toHaveBeenCalledWith('bob');
  expect(api.joinVoiceChannel).not.toHaveBeenCalled();
});

it('a card shows the caller display name I know, or the name the server sent', () => {
  setKnownUsers([{ id: 'alice', username: 'alice', display_name: 'Алиса', role: 'member' } as never]);
  sessionHandlers['flock-call'](call);
  expect(pendingCalls()[0].from_name).toBe('Алиса');
  dismissCall(pendingCalls()[0].id);
  sessionHandlers['flock-call']({ ...call, from_user_id: 'stranger', from_name: 'Stranger' });
  expect(pendingCalls()[0].from_name).toBe('Stranger');
  setKnownUsers([]);
});

it('a call the server failed to send does not move me', async () => {
  vi.mocked(api.callFlock).mockRejectedValueOnce(new Error('Not connected via QUIC'));
  await callFlock(lounge);
  expect(api.joinVoiceChannel).not.toHaveBeenCalled();
  expect(appError()).toContain('Not connected');
});

it('a refused call says why: too soon, offline, or the server text', async () => {
  await callUserHere('bob');
  expect(handleFlockRefusal({ code: 429, message: 'Wait 10 s' })).toBe(true);
  expect(appError()).toBe(t('Wait a moment before calling again'));
  await callUserHere('bob');
  expect(handleFlockRefusal({ code: 404, message: 'That person is offline' })).toBe(true);
  expect(appError()).toBe(t('This person is offline'));
  await callUserHere('bob');
  expect(handleFlockRefusal({ code: 400, message: 'Join a voice room first' })).toBe(true);
  expect(appError()).toBe('400: Join a voice room first');
});

it('a server error long after a call is not a refusal of that call', async () => {
  await callUserHere('bob');
  vi.advanceTimersByTime(60_000);
  expect(handleFlockRefusal({ code: 429, message: 'Wait' })).toBe(false);
});
