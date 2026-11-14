import { afterEach, expect, it, vi } from 'vitest';
import { api } from '../services/api';
import { activeVoiceChannel, joinVoice, setActiveVoiceChannel } from './voiceStore';
import { appError, setAppError } from './notificationStore';
import { sessionHandlers } from './sessionStore';
import { startSharing } from './screenStore';
import { isSharing } from './screenState';

afterEach(() => vi.restoreAllMocks());

const room = { id: 'voice', short_id: 1, name: 'Room', channel_type: 'voice', position: 0, bitrate_kbps: 64 } as const;
const refusal = { code: 429, message: 'Wait 3 s; too many voice changes' };

it('rolls back a join the server refused right after it was sent', async () => {
  localStorage.clear(); setActiveVoiceChannel(null); setAppError(null);
  vi.spyOn(api, 'joinVoiceChannel').mockResolvedValue(undefined);
  const leave = vi.spyOn(api, 'leaveVoiceChannel').mockResolvedValue(undefined);
  await joinVoice(room);
  expect(activeVoiceChannel()?.id).toBe('voice');

  sessionHandlers['server-error'](refusal);

  expect(leave).toHaveBeenCalled();
  await vi.waitFor(() => expect(activeVoiceChannel()).toBeNull());
  expect(appError()).toBe('Too many changes. Wait 3 s');
});

it('keeps the join when the refusal does not come right after it', async () => {
  localStorage.clear(); setActiveVoiceChannel(null); setAppError(null);
  vi.spyOn(api, 'joinVoiceChannel').mockResolvedValue(undefined);
  const leave = vi.spyOn(api, 'leaveVoiceChannel').mockResolvedValue(undefined);
  await joinVoice(room);
  vi.spyOn(Date, 'now').mockReturnValue(Date.now() + 60_000);

  sessionHandlers['server-error'](refusal);

  expect(leave).not.toHaveBeenCalled();
  expect(activeVoiceChannel()?.id).toBe('voice');
});

it('stops a share the server refused right after it was started', async () => {
  localStorage.clear(); setAppError(null);
  vi.spyOn(api, 'startScreenShare').mockResolvedValue(1);
  const stop = vi.spyOn(api, 'stopScreenShare').mockResolvedValue(undefined);
  expect(await startSharing('monitor', 1)).toBeNull();
  expect(isSharing()).toBe(true);

  sessionHandlers['server-error'](refusal);

  expect(stop).toHaveBeenCalled();
  expect(isSharing()).toBe(false);
});

it('leaves other server errors to the existing handling', () => {
  setAppError(null);
  sessionHandlers['server-error']({ code: 400, message: 'Join a voice room first' });
  expect(appError()).toBe('400: Join a voice room first');
});
