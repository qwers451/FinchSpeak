import { beforeEach, expect, it, vi } from 'vitest';
import { api } from '../services/api';
import { appError, setAppError } from './notificationStore';
import {
  changeVoiceSettings, choosePushToTalkKey, DEFAULT_VOICE_SETTINGS, pushToTalkKey, restoreVoiceSettings,
  setPushToTalkKey, setVoiceSettings, shortcutFromEvent, voiceSettings,
} from './voiceStore';

const press = (code: string, mods: Partial<{ ctrlKey: boolean; altKey: boolean; shiftKey: boolean; metaKey: boolean }> = {}) =>
  shortcutFromEvent({ code, ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, ...mods });

beforeEach(() => {
  vi.restoreAllMocks();
  localStorage.clear();
  setAppError(null);
  setVoiceSettings(DEFAULT_VOICE_SETTINGS);
  setPushToTalkKey(null);
});

it('AUD-042 key presses become shortcut text, modifiers alone do not', () => {
  expect(press('F8')).toBe('F8');
  expect(press('KeyK', { ctrlKey: true, shiftKey: true })).toBe('Control+Shift+K');
  expect(press('Digit5')).toBe('5');
  expect(press('Space', { altKey: true })).toBe('Alt+Space');
  expect(press('ControlLeft', { ctrlKey: true })).toBeNull();
  expect(press('ShiftRight', { shiftKey: true })).toBeNull();
});

it('AUD-042 a change is sent to the backend, remembered, and merged with the rest', async () => {
  const send = vi.spyOn(api, 'setVoiceSettings').mockResolvedValue();
  await changeVoiceSettings({ input_gain: 2 });
  await changeVoiceSettings({ monitor: true });
  expect(send).toHaveBeenLastCalledWith({ ...DEFAULT_VOICE_SETTINGS, input_gain: 2, monitor: true });
  expect(voiceSettings()).toMatchObject({ input_gain: 2, monitor: true });
});

it('AUD-042 the backend refusing a change leaves the old values and shows why', async () => {
  vi.spyOn(api, 'setVoiceSettings').mockRejectedValue('nope');
  await changeVoiceSettings({ input_gain: 3 });
  expect(voiceSettings().input_gain).toBe(1);
  expect(appError()).toContain('nope');
});

it('AUD-042 push-to-talk cannot be switched on without a key', async () => {
  const send = vi.spyOn(api, 'setVoiceSettings').mockResolvedValue();
  await changeVoiceSettings({ push_to_talk: true });
  expect(send).not.toHaveBeenCalled();
  expect(appError()).toContain('key');

  vi.spyOn(api, 'setPushToTalkKey').mockResolvedValue();
  await choosePushToTalkKey('F8');
  await changeVoiceSettings({ push_to_talk: true });
  expect(voiceSettings().push_to_talk).toBe(true);
  expect(pushToTalkKey()).toBe('F8');
});

it('AUD-042 removing the key also switches push-to-talk off', async () => {
  vi.spyOn(api, 'setVoiceSettings').mockResolvedValue();
  vi.spyOn(api, 'setPushToTalkKey').mockResolvedValue();
  await choosePushToTalkKey('F8');
  await changeVoiceSettings({ push_to_talk: true });
  await choosePushToTalkKey(null);
  expect(pushToTalkKey()).toBeNull();
  expect(voiceSettings().push_to_talk).toBe(false);
});

it('AUD-042 a key the system refuses is reported and not remembered', async () => {
  vi.spyOn(api, 'setPushToTalkKey').mockRejectedValue('already used by another program');
  await choosePushToTalkKey('F8');
  expect(pushToTalkKey()).toBeNull();
  expect(appError()).toContain('already used');
});

it('AUD-042 settings and key come back after a restart', async () => {
  vi.spyOn(api, 'setVoiceSettings').mockResolvedValue();
  const key = vi.spyOn(api, 'setPushToTalkKey').mockResolvedValue();
  await choosePushToTalkKey('F9');
  await changeVoiceSettings({ push_to_talk: true, input_gain: 1.5 });

  setVoiceSettings(DEFAULT_VOICE_SETTINGS);
  setPushToTalkKey(null);
  key.mockClear();
  await restoreVoiceSettings();
  expect(key).toHaveBeenCalledWith('F9');
  expect(voiceSettings()).toMatchObject({ push_to_talk: true, input_gain: 1.5, key_down: false });
});

it('AUD-042 corrupt saved settings fall back to the defaults', async () => {
  localStorage.setItem('finchspeak.voiceSettings', '{"settings":{"input_gain":"loud"},"key":42}');
  const send = vi.spyOn(api, 'setVoiceSettings').mockResolvedValue();
  await restoreVoiceSettings();
  expect(send).toHaveBeenCalledWith({ ...DEFAULT_VOICE_SETTINGS });
});

it('enhancement migrates old preferences without losing existing values', async () => {
  localStorage.setItem('finchspeak.voiceSettings', JSON.stringify({ settings: { input_gain: 1.5, vad_threshold_db: -52, push_to_talk: true, monitor: true }, key: 'F9' }));
  const send = vi.spyOn(api, 'setVoiceSettings').mockResolvedValue();
  vi.spyOn(api, 'setPushToTalkKey').mockResolvedValue();
  await restoreVoiceSettings();
  expect(send).toHaveBeenCalledWith({ input_gain: 1.5, vad_threshold_db: -52, push_to_talk: true, monitor: true, key_down: false, voice_enhancement: true });
  expect(pushToTalkKey()).toBe('F9');
});

it.each([undefined, 'bad', null, false, true])('enhancement restores saved %s safely', async (value) => {
  localStorage.setItem('finchspeak.voiceSettings', JSON.stringify({ settings: { voice_enhancement: value } }));
  vi.spyOn(api, 'setVoiceSettings').mockResolvedValue();
  await restoreVoiceSettings();
  expect(voiceSettings().voice_enhancement).toBe(typeof value === 'boolean' ? value : true);
});

it('enhancement false survives restart and rejected changes preserve storage', async () => {
  const send = vi.spyOn(api, 'setVoiceSettings').mockResolvedValue();
  await changeVoiceSettings({ voice_enhancement: false });
  const saved = localStorage.getItem('finchspeak.voiceSettings');
  setVoiceSettings(DEFAULT_VOICE_SETTINGS);
  await restoreVoiceSettings();
  expect(voiceSettings().voice_enhancement).toBe(false);
  send.mockRejectedValue('unavailable');
  await changeVoiceSettings({ voice_enhancement: true });
  expect(voiceSettings().voice_enhancement).toBe(false);
  expect(localStorage.getItem('finchspeak.voiceSettings')).toBe(saved);
  expect(appError()).toContain('unavailable');
});

it('serializes rapid changes and merges them after the preceding acknowledgement', async () => {
  let acknowledge!: () => void;
  const send = vi.spyOn(api, 'setVoiceSettings').mockImplementationOnce(() => new Promise<void>((resolve) => { acknowledge = resolve; })).mockResolvedValue();
  const first = changeVoiceSettings({ input_gain: 2 });
  const second = changeVoiceSettings({ voice_enhancement: false });
  await Promise.resolve();
  expect(send).toHaveBeenCalledTimes(1);
  acknowledge();
  await Promise.all([first, second]);
  expect(send).toHaveBeenLastCalledWith({ ...DEFAULT_VOICE_SETTINGS, input_gain: 2, voice_enhancement: false });
  expect(voiceSettings()).toMatchObject({ input_gain: 2, voice_enhancement: false });
});
