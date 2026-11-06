import { beforeEach, expect, it, vi } from 'vitest';
import { api } from '../services/api';
import { appError, setAppError } from './notificationStore';
import { clearToasts, toasts } from '../ui';
import {
  changeVoiceSettings, chooseHotkey, choosePushToTalkKey, deafenKey, DEFAULT_VOICE_SETTINGS, handleHotkey, isDeafened, isMuted, muteKey,
  pushToTalkKey, restoreVoiceSettings, setActiveVoiceChannel, setDeafenKey, setIsDeafened, setIsMuted, setMuteKey, setPushToTalkKey, setVoiceSettings,
  shortcutFromEvent, hotkeyErrorText, voiceSettings,
} from './voiceStore';
import { DEFAULT_PREFS, setPrefs } from './settingsStore';

const press = (code: string, mods: Partial<{ ctrlKey: boolean; altKey: boolean; shiftKey: boolean; metaKey: boolean }> = {}) =>
  shortcutFromEvent({ code, ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, ...mods });

beforeEach(() => {
  vi.restoreAllMocks();
  localStorage.clear();
  setAppError(null);
  clearToasts();
  setActiveVoiceChannel(null);
  setVoiceSettings(DEFAULT_VOICE_SETTINGS);
  setPushToTalkKey(null);
  setMuteKey(null);
  setDeafenKey(null);
  setIsMuted(false);
  setIsDeafened(false);
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

it('mute and deafen keys are sent, remembered and restored', async () => {
  const send = vi.spyOn(api, 'setHotkey').mockResolvedValue();
  await chooseHotkey('toggle_mute', 'F9');
  await chooseHotkey('toggle_deafen', 'Control+F10');
  expect(send).toHaveBeenCalledWith('toggle_mute', 'F9');
  expect(muteKey()).toBe('F9');
  setMuteKey(null);
  setDeafenKey(null);
  send.mockClear();
  vi.spyOn(api, 'setVoiceSettings').mockResolvedValue();
  await restoreVoiceSettings();
  expect(send).toHaveBeenCalledWith('toggle_mute', 'F9');
  expect(send).toHaveBeenCalledWith('toggle_deafen', 'Control+F10');
  expect(deafenKey()).toBe('Control+F10');
});

it('one key cannot serve two actions', async () => {
  vi.spyOn(api, 'setPushToTalkKey').mockResolvedValue();
  const send = vi.spyOn(api, 'setHotkey').mockResolvedValue();
  await choosePushToTalkKey('F8');
  await chooseHotkey('toggle_mute', 'F8');
  expect(send).not.toHaveBeenCalled();
  expect(muteKey()).toBeNull();
  expect(appError()).toContain('F8');
});

const room = { id: 'room', short_id: 2, name: 'Room', channel_type: 'voice' as const, position: 0, bitrate_kbps: 64 };

it('a pressed mute or deafen key toggles the microphone or the sound', async () => {
  setActiveVoiceChannel(room);
  const state = vi.spyOn(api, 'setVoiceState').mockResolvedValue();
  await handleHotkey('toggle_mute');
  expect(isMuted()).toBe(true);
  await handleHotkey('toggle_mute');
  expect(isMuted()).toBe(false);
  await handleHotkey('toggle_deafen');
  expect(isDeafened()).toBe(true);
  expect(isMuted()).toBe(true);
  expect(state).toHaveBeenLastCalledWith(true, true);
  await handleHotkey('toggle_deafen');
  expect(isDeafened()).toBe(false);
  expect(isMuted()).toBe(false);
});

it('mute hotkey outside a room is ignored', async () => {
  const state = vi.spyOn(api, 'setVoiceState').mockResolvedValue();
  await handleHotkey('toggle_mute');
  await handleHotkey('toggle_deafen');
  expect(isMuted()).toBe(false);
  expect(isDeafened()).toBe(false);
  expect(state).not.toHaveBeenCalled();
  expect(appError()).toBeNull();
  expect(toasts()).toHaveLength(0);
});

it('a key that fails to register does not stop the other keys or the settings', async () => {
  localStorage.setItem('finchspeak.voiceSettings', JSON.stringify({
    settings: { input_gain: 1.5, vad_threshold_db: -45, push_to_talk: false, monitor: false, voice_enhancement: true },
    key: null, muteKey: 'F9', deafenKey: 'F10',
  }));
  const saved = localStorage.getItem('finchspeak.voiceSettings');
  const hotkey = vi.spyOn(api, 'setHotkey').mockRejectedValueOnce('taken').mockResolvedValue();
  const send = vi.spyOn(api, 'setVoiceSettings').mockResolvedValue();
  await restoreVoiceSettings();
  expect(hotkey).toHaveBeenCalledWith('toggle_mute', 'F9');
  expect(hotkey).toHaveBeenCalledWith('toggle_deafen', 'F10');
  expect(send).toHaveBeenCalledWith(expect.objectContaining({ input_gain: 1.5 }));
  expect(toasts().map((toast) => toast.title)).toEqual(['Could not use F9 for "Mute microphone": another program may be using it.']);
  await changeVoiceSettings({ monitor: true });
  expect(JSON.parse(localStorage.getItem('finchspeak.voiceSettings')!)).toMatchObject({ muteKey: 'F9', deafenKey: 'F10' });
  expect(JSON.parse(saved!)).toMatchObject({ muteKey: 'F9', deafenKey: 'F10' }); // what was stored is what was kept
});

it('backend key errors are shown in Russian', () => {
  setPrefs({ ...DEFAULT_PREFS, appearance: { ...DEFAULT_PREFS.appearance, language: 'ru' } });
  try {
    expect(hotkeyErrorText('Error: "Foo" cannot be used as a key: bad name')).toBe('«Foo» нельзя использовать как клавишу: bad name');
    expect(hotkeyErrorText('Could not use "F9" as a key: taken')).toBe('Не удалось использовать «F9» как клавишу: taken');
    expect(hotkeyErrorText('"F9" is already used by another action')).toBe('Клавиша «F9» уже используется для другого действия');
    expect(hotkeyErrorText('Keys are unavailable')).toBe('Клавиши сейчас недоступны');
  } finally {
    setPrefs(DEFAULT_PREFS);
  }
});

it('a failed push-to-talk key is pushed as off but the preference stays stored', async () => {
  const stored = JSON.stringify({
    settings: { input_gain: 1, vad_threshold_db: -45, push_to_talk: true, monitor: false, voice_enhancement: true },
    key: 'F8', muteKey: null, deafenKey: null,
  });
  localStorage.setItem('finchspeak.voiceSettings', stored);
  vi.spyOn(api, 'setPushToTalkKey').mockRejectedValue('taken');
  const send = vi.spyOn(api, 'setVoiceSettings').mockResolvedValue();
  await restoreVoiceSettings();
  expect(send).toHaveBeenCalledWith(expect.objectContaining({ push_to_talk: false }));
  expect(toasts()).toHaveLength(1);
  expect(pushToTalkKey()).toBe('F8');
  await changeVoiceSettings({ monitor: true });
  expect(send).toHaveBeenLastCalledWith(expect.objectContaining({ push_to_talk: false, monitor: true }));
  expect(JSON.parse(localStorage.getItem('finchspeak.voiceSettings')!)).toMatchObject({ key: 'F8', settings: { push_to_talk: true } });
  // a successful rebind brings push-to-talk back on the backend
  vi.spyOn(api, 'setPushToTalkKey').mockResolvedValue();
  await choosePushToTalkKey('F7');
  expect(send).toHaveBeenLastCalledWith(expect.objectContaining({ push_to_talk: true }));
});
