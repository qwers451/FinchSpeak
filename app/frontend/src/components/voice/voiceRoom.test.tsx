import { render } from 'solid-js/web';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(async () => undefined), Channel: class { onmessage: unknown = null; } }));

import { api } from '../../services/api';
import { setCurrentUser } from '../../stores/authStore';
import { setWatching } from '../../stores/screenState';
import { sessionHandlers, setKnownUsers, setLocalPeerId, setConnectionStatus, resetSessionState } from '../../stores/sessionStore';
import {
  availableDevices, setActiveVoiceChannel, setAvailableDevices, setIsDeafened, setIsLocalSpeaking, setIsMuted, setMicLevelDb, setMicTestPhase,
  setPushToTalkKey, setSelectedDevices, setSpeakingPeers, setVoiceParticipants, setVoiceSettings, DEFAULT_VOICE_SETTINGS,
} from '../../stores/voiceStore';
import { setVolumes } from '../../stores/volumeStore';
import { clearToasts } from '../../ui';
import { VoicePanel } from '../VoicePanel';
import { VoiceSettingsModal } from './VoiceSettingsModal';

const me = { id: 'me', username: 'alice', display_name: 'Alice Cooper', role: 'member' as const };
const bob = { id: 'bob', username: 'bob', display_name: 'Bob Marley', role: 'member' as const };
const dave = { id: 'dave', username: 'dave', display_name: 'Dave Okoro', role: 'admin' as const };
const person = (peer: number, user: { id: string; username: string }, extra = {}) => ({ user_id: user.id, peer_id: peer, username: user.username, channel_id: 'room', ...extra });

let dispose: (() => void) | undefined;
const mount = (view: () => unknown) => {
  const host = document.createElement('div');
  document.body.appendChild(host);
  dispose = render(view as () => never, host);
  return host;
};
const flush = () => new Promise((resolve) => setTimeout(resolve, 0));
const byLabel = (host: ParentNode, label: string) => host.querySelector<HTMLElement>(`[aria-label="${label}"]`)!;

beforeEach(() => {
  vi.restoreAllMocks();
  localStorage.clear();
  resetSessionState();
  clearToasts();
  setVolumes({});
  setCurrentUser(me);
  setKnownUsers([me, bob, dave]);
  setLocalPeerId(1);
  setConnectionStatus('connected');
  setActiveVoiceChannel({ id: 'room', short_id: 2, name: 'General Voice', channel_type: 'voice', position: 0, bitrate_kbps: 64 });
  setVoiceParticipants([person(1, me), person(2, bob, { is_muted: true }), person(3, dave)]);
  setSpeakingPeers(new Set<number>());
  setIsLocalSpeaking(false);
  setIsMuted(false);
  setIsDeafened(false);
  setWatching(null);
  setVoiceSettings(DEFAULT_VOICE_SETTINGS);
  setPushToTalkKey(null);
  setMicLevelDb(-70);
  setMicTestPhase('idle');
});
afterEach(() => {
  dispose?.();
  dispose = undefined;
  document.body.innerHTML = '';
});

it('shows everyone in the room with me first, and who is muted or speaking', () => {
  setSpeakingPeers(new Set([3]));
  const host = mount(() => <VoicePanel />);
  const names = Array.from(host.querySelectorAll('.room-grid .vc')).map((tile) => tile.getAttribute('aria-label'));
  expect(names).toEqual(['Alice Cooper', 'Bob Marley, muted', 'Dave Okoro']);
  expect(host.querySelector('[data-peer-id="3"]')?.classList.contains('speaking')).toBe(true);
  expect(host.querySelector('[data-peer-id="1"] .vol')).toBeNull(); // no volume slider for myself
  expect(host.querySelector('.mhead')?.textContent).toContain('3 in room');
  expect(host.querySelector('[data-peer-id="3"]')?.textContent).toContain('Admin');
});

it('a volume slider sets the mixer and is remembered for next time', async () => {
  const mixer = vi.spyOn(api, 'setSpeakerVolume').mockResolvedValue();
  const host = mount(() => <VoicePanel />);
  const slider = byLabel(host, 'Volume of Bob Marley') as HTMLInputElement;
  slider.value = '1.5';
  slider.dispatchEvent(new Event('input', { bubbles: true }));
  await flush();
  expect(mixer).toHaveBeenCalledWith(2, 1.5);
  expect(host.querySelector('[data-peer-id="2"] .vol-num')?.textContent).toBe('150%');
  expect(JSON.parse(localStorage.getItem('finchspeak.volumes')!)).toEqual({ bob: 1.5 });
});

it('poor link quality is shown on the person it concerns', () => {
  const host = mount(() => <VoicePanel />);
  sessionHandlers['voice-quality']([{ peer_id: 2, quality: 'poor', loss: 0.2 }]);
  expect(host.querySelector('[data-peer-id="2"]')?.textContent).toContain('Poor connection');
  expect(host.querySelector('[data-peer-id="3"]')?.textContent).not.toContain('Poor connection');
});

it('a sharer gets a LIVE card with a Watch button, and the button starts watching', async () => {
  const subscribe = vi.spyOn(api, 'subscribeScreenShare').mockResolvedValue();
  setVoiceParticipants([person(1, me), person(2, bob, { is_sharing: true })]);
  const host = mount(() => <VoicePanel />);
  const card = host.querySelector('.live-card')!;
  expect(card.textContent).toContain('Bob Marley');
  expect(card.textContent).toContain('Live');
  Array.from(card.querySelectorAll('button')).find((button) => button.textContent?.includes('Watch'))!.click();
  await flush();
  expect(subscribe).toHaveBeenCalledWith('bob', 2);
});

it('a crowded room shows twelve tiles and the rest in a roster, speakers first', () => {
  const crowd = Array.from({ length: 16 }, (_, i) => person(i + 10, { id: `x${i}`, username: `guest${i}` }));
  setVoiceParticipants([person(1, me), ...crowd]);
  setSpeakingPeers(new Set([25]));
  const host = mount(() => <VoicePanel />);
  expect(host.querySelectorAll('.room-grid .vc')).toHaveLength(12);
  expect(host.querySelector('.roster')?.textContent).toContain('+ 5 more');
  expect(host.querySelector('.room-grid [data-peer-id="25"]')).not.toBeNull();
});

it('the dock mutes, deafens and leaves', async () => {
  const state = vi.spyOn(api, 'setVoiceState').mockResolvedValue();
  const leave = vi.spyOn(api, 'leaveVoiceChannel').mockResolvedValue();
  const host = mount(() => <VoicePanel />);
  byLabel(host, 'Mute').click();
  await flush();
  expect(state).toHaveBeenCalledWith(true, false);
  expect(byLabel(host, 'Unmute').getAttribute('aria-pressed')).toBe('true');
  byLabel(host, 'Leave the room').click();
  await flush();
  expect(leave).toHaveBeenCalled();
});

it('shows the push-to-talk key and whether it is held', () => {
  setPushToTalkKey('F8');
  setVoiceSettings({ ...DEFAULT_VOICE_SETTINGS, push_to_talk: true, key_down: false });
  const host = mount(() => <VoicePanel />);
  expect(host.textContent).toContain('F8 · push to talk');
  setVoiceSettings({ ...DEFAULT_VOICE_SETTINGS, push_to_talk: true, key_down: true });
  expect(host.textContent).toContain('F8 · talking');
});

it('the settings button opens the voice settings, and Esc closes them', async () => {
  vi.spyOn(api, 'getAudioDevices').mockResolvedValue({ inputs: [], outputs: [], selected_input: null, selected_output: null });
  const host = mount(() => <VoicePanel />);
  byLabel(host, 'Voice settings').click();
  await flush();
  expect(document.querySelector('[role="dialog"]')?.textContent).toContain('Voice and audio');
  document.querySelector('.overlay')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
  await flush();
  expect(document.querySelector('[role="dialog"]')).toBeNull();
});

// ---- voice settings (D3)

const devices = {
  inputs: [{ name: 'Built-in mic', is_default: true }, { name: 'Headset mic', is_default: false }],
  outputs: [{ name: 'Speakers', is_default: true }, { name: 'Headset', is_default: false }],
  selected_input: null,
  selected_output: 'Headset',
};

it('lists every device, marks the default and the one in use, and applies a change at once', async () => {
  vi.spyOn(api, 'getAudioDevices').mockResolvedValue(devices);
  const apply = vi.spyOn(api, 'setAudioDevices').mockResolvedValue();
  setSelectedDevices({ input: null, output: null });
  const host = mount(() => <VoiceSettingsModal open onClose={() => {}} />);
  await flush();
  const [input, output] = Array.from(document.querySelectorAll('select')) as HTMLSelectElement[];
  expect(Array.from(input.options).map((o) => o.textContent)).toEqual(['System default', 'Built-in mic (default)', 'Headset mic']);
  expect(output.value).toBe('Headset');
  input.value = 'Headset mic';
  input.dispatchEvent(new Event('change', { bubbles: true }));
  await flush();
  expect(apply).toHaveBeenCalledWith('Headset mic', 'Headset');
  expect(host).toBeTruthy();
});

it('warns when the chosen device is gone and the default is used instead', async () => {
  vi.spyOn(api, 'getAudioDevices').mockResolvedValue({ ...devices, selected_input: 'Old USB mic' });
  mount(() => <VoiceSettingsModal open onClose={() => {}} />);
  await flush();
  expect(document.body.textContent).toContain('This device is not connected; the system default is used instead.');
  expect(availableDevices().inputs).toHaveLength(2);
});

it('the meter follows the microphone and the threshold line follows the slider', async () => {
  vi.spyOn(api, 'getAudioDevices').mockResolvedValue(devices);
  mount(() => <VoiceSettingsModal open onClose={() => {}} />);
  await flush();
  const meter = document.querySelector('.meter')!;
  const fill = () => (meter.querySelector('i') as HTMLElement).style.width;
  expect(fill()).toBe('0%');
  setMicLevelDb(-35);
  expect(fill()).toBe('50%');
  setVoiceSettings({ ...DEFAULT_VOICE_SETTINGS, vad_threshold_db: -45 });
  expect((meter.querySelector('.thr') as HTMLElement).style.left).toBe('36%');
  expect(document.body.textContent).toContain('above the line');
  setIsMuted(true);
  expect(fill()).toBe('0%');
  expect(document.body.textContent).toContain('You are muted');
});

it('the microphone test asks the backend and is busy while it runs', async () => {
  vi.spyOn(api, 'getAudioDevices').mockResolvedValue(devices);
  const test = vi.spyOn(api, 'testMicrophone').mockResolvedValue();
  mount(() => <VoiceSettingsModal open onClose={() => {}} />);
  await flush();
  const button = Array.from(document.querySelectorAll('button')).find((b) => b.textContent?.includes('Test microphone'))!;
  button.click();
  await flush();
  expect(test).toHaveBeenCalledWith(3);
  sessionHandlers['mic-test']({ phase: 'recording' });
  expect((Array.from(document.querySelectorAll('button')).find((b) => b.textContent?.includes('Recording'))! as HTMLButtonElement).disabled).toBe(true);
  sessionHandlers['mic-test']({ phase: 'done' });
  expect(document.body.textContent).toContain('Test microphone');
});

it('captures a push-to-talk key, Esc cancels, and "hear myself" warns about headphones', async () => {
  vi.spyOn(api, 'getAudioDevices').mockResolvedValue(devices);
  const key = vi.spyOn(api, 'setPushToTalkKey').mockResolvedValue();
  vi.spyOn(api, 'setVoiceSettings').mockResolvedValue();
  mount(() => <VoiceSettingsModal open onClose={() => {}} />);
  await flush();
  const capture = document.querySelector<HTMLButtonElement>('[aria-label="Push-to-talk key"]')!;
  capture.click();
  await flush();
  expect(capture.textContent).toContain('Press a key');
  capture.dispatchEvent(new KeyboardEvent('keydown', { code: 'Escape', key: 'Escape', bubbles: true, cancelable: true }));
  await flush();
  expect(capture.textContent).toContain('Choose key');
  capture.click();
  capture.dispatchEvent(new KeyboardEvent('keydown', { code: 'KeyV', key: 'v', ctrlKey: true, bubbles: true, cancelable: true }));
  await flush();
  expect(key).toHaveBeenCalledWith('Control+V');

  const hear = document.querySelector<HTMLElement>('[aria-label="Hear myself"]')!;
  expect(document.body.textContent).not.toContain('Use headphones');
  hear.click();
  await flush();
  expect(document.body.textContent).toContain('Use headphones');
});
