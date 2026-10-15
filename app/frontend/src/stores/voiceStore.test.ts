import { beforeEach, expect, it, vi } from 'vitest';
import { api } from '../services/api';
import { setActiveVoiceChannel, setVoiceParticipants, voiceParticipants, toggleDeafen, isMuted, setIsMuted, setIsDeafened,
  availableDevices, selectedDevices, setSelectedDevices, loadAudioDevices, chooseAudioDevice, joinVoice } from './voiceStore';
import { appError, setAppError } from './notificationStore';
import { sessionHandlers } from './sessionStore';

beforeEach(() => {
  vi.restoreAllMocks();
  localStorage.clear();
  setSelectedDevices({ input: null, output: null });
  setAppError(null);
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

// --- AUD-024: choosing the microphone and speakers ---

const devices = {
  inputs: [{ name: 'Built-in mic', is_default: true }, { name: 'Headset mic', is_default: false }],
  outputs: [{ name: 'Speakers', is_default: true }, { name: 'Headset', is_default: false }],
  selected_input: null,
  selected_output: 'Headset',
};

it('AUD-024 lists the devices and shows what the backend uses', async () => {
  vi.spyOn(api, 'getAudioDevices').mockResolvedValue(devices);
  await loadAudioDevices();
  expect(availableDevices().inputs.map((d) => d.name)).toEqual(['Built-in mic', 'Headset mic']);
  expect(selectedDevices()).toEqual({ input: null, output: 'Headset' });
});

it('AUD-024 applies a choice at once and remembers it', async () => {
  const apply = vi.spyOn(api, 'setAudioDevices').mockResolvedValue();
  await chooseAudioDevice('input', 'Headset mic');
  expect(apply).toHaveBeenCalledWith('Headset mic', null);
  expect(selectedDevices()).toEqual({ input: 'Headset mic', output: null });
  expect(JSON.parse(localStorage.getItem('finchspeak.audioDevices') ?? 'null')).toEqual({ input: 'Headset mic', output: null });

  await chooseAudioDevice('output', 'Headset');
  expect(apply).toHaveBeenLastCalledWith('Headset mic', 'Headset');
});

it('AUD-024 choosing the system default clears the choice', async () => {
  vi.spyOn(api, 'setAudioDevices').mockResolvedValue();
  await chooseAudioDevice('output', 'Headset');
  await chooseAudioDevice('output', null);
  expect(selectedDevices().output).toBeNull();
});

it('AUD-024 keeps the old choice and says why when the backend refuses', async () => {
  vi.spyOn(api, 'setAudioDevices').mockRejectedValue(new Error('no such device'));
  await chooseAudioDevice('input', 'Headset mic');
  expect(selectedDevices().input).toBeNull();
  expect(appError()).toContain('no such device');
});

it('AUD-024 re-applies the remembered devices when joining a voice channel', async () => {
  localStorage.setItem('finchspeak.audioDevices', JSON.stringify({ input: 'Headset mic', output: 'Headset' }));
  const order: string[] = [];
  vi.spyOn(api, 'setAudioDevices').mockImplementation(async () => { order.push('devices'); });
  vi.spyOn(api, 'joinVoiceChannel').mockImplementation(async () => { order.push('join'); });
  await joinVoice({ id: 'v', short_id: 2, name: 'voice', channel_type: 'voice', position: 1, bitrate_kbps: 64 });
  expect(order).toEqual(['devices', 'join']);
  expect(selectedDevices()).toEqual({ input: 'Headset mic', output: 'Headset' });
});

it('AUD-024 still works when browser storage is unavailable', async () => {
  vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => { throw new Error('blocked'); });
  vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => { throw new Error('blocked'); });
  vi.spyOn(api, 'setAudioDevices').mockResolvedValue();
  vi.spyOn(api, 'joinVoiceChannel').mockResolvedValue();
  await chooseAudioDevice('output', 'Headset');
  expect(selectedDevices().output).toBe('Headset');
  await joinVoice({ id: 'v', short_id: 2, name: 'voice', channel_type: 'voice', position: 1, bitrate_kbps: 64 });
});

it('AUD-024 tells the user when a device disappears or is replaced by the default', () => {
  sessionHandlers['audio-device-lost'](null);
  expect(appError()).toMatch(/disconnected/i);
  sessionHandlers['audio-device-fallback']({ kind: 'output', wanted: 'Headset' });
  expect(appError()).toContain('Headset');
  expect(appError()).toMatch(/default/i);
  sessionHandlers['audio-device-failed'](null);
  expect(appError()).toMatch(/rejoin/i);
});
