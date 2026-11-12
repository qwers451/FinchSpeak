import { createSignal } from 'solid-js';
import { render } from 'solid-js/web';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';

const core = vi.hoisted(() => ({ invoke: vi.fn(async () => undefined) }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: core.invoke, Channel: class { onmessage: unknown = null; } }));

import { api } from '../../services/api';
import { setCurrentUser } from '../../stores/authStore';
import { setScreenAudioSource, endedShare, setEndedShare, setIsSharing, setScreenSources, setSharingInfo, setShareStats, setWatching } from '../../stores/screenState';
import { formatBitrate } from '../../i18n';
import { resetSessionState, setConnectionStatus, setKnownUsers } from '../../stores/sessionStore';
import { setVoiceParticipants } from '../../stores/voiceStore';
import { setVolumes } from '../../stores/volumeStore';
import { DEFAULT_PREFS, setPrefs } from '../../stores/settingsStore';
import { clearToasts } from '../../ui';
import { monitorName, SharePickerModal } from './SharePickerModal';
import { SharerPill } from './SharerPill';
import { EndedPanel, ViewerPanel } from './ViewerPanel';

let dispose: (() => void) | undefined;
const mount = (view: () => unknown) => {
  const host = document.createElement('div');
  document.body.appendChild(host);
  dispose = render(view as () => never, host);
  return host;
};
const flush = () => new Promise((resolve) => setTimeout(resolve, 0));
const button = (text: string) => Array.from(document.querySelectorAll('button')).find((b) => b.textContent?.includes(text)) as HTMLButtonElement;
const shareAudioToggle = () => document.querySelector<HTMLButtonElement>('[aria-label="Share system sound too"]')!;

const sources = {
  monitors: [
    { id: 1, name: 'DISPLAY1', width: 2560, height: 1440, is_primary: true },
    { id: 2, name: 'DISPLAY2', width: 1920, height: 1080, is_primary: false },
  ],
  windows: [{ id: 9, title: 'Rocket League', app_name: 'RocketLeague.exe', width: 1920, height: 1080 }],
};

beforeEach(() => {
  setPrefs({ ...DEFAULT_PREFS, appearance: { ...DEFAULT_PREFS.appearance, language: 'en' } });
  vi.restoreAllMocks();
  core.invoke.mockClear();
  localStorage.clear();
  resetSessionState();
  clearToasts();
  setVolumes({});
  setCurrentUser({ id: 'me', username: 'alice', display_name: 'Alice', role: 'member' });
  setKnownUsers([]);
  setConnectionStatus('connected');
  setScreenSources({ monitors: [], windows: [] });
  setWatching(null);
  setEndedShare(null);
  setIsSharing(false);
  setSharingInfo(null);
  setShareStats(null);
  setVoiceParticipants([{ user_id: 'carol', peer_id: 5, username: 'Carol', channel_id: 'room' }]);
  vi.spyOn(api, 'listScreenSources').mockResolvedValue(sources);
  vi.spyOn(api, 'captureThumbnail').mockResolvedValue(new Uint8Array([0xff, 0xd8, 0xff]).buffer as ArrayBuffer);
  vi.stubGlobal('URL', { createObjectURL: () => 'blob:preview', revokeObjectURL: vi.fn() });
});
afterEach(() => {
  dispose?.();
  dispose = undefined;
  document.body.innerHTML = '';
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

// ---- picker (E1/E2)

it('lists screens with name, size and a Main badge, selects the main one, and shows live previews', async () => {
  mount(() => <SharePickerModal open onClose={() => {}} />);
  await flush();
  await flush();
  const tiles = Array.from(document.querySelectorAll('[role="option"]'));
  expect(tiles.map((tile) => tile.querySelector('strong')?.textContent)).toEqual(['DISPLAY1', 'DISPLAY2']);
  expect(tiles[0].textContent).toContain('2560 × 1440');
  expect(tiles[0].textContent).toContain('Main');
  expect(tiles[0].getAttribute('aria-selected')).toBe('true');
  expect(tiles[1].getAttribute('aria-selected')).toBe('false');
  await new Promise((resolve) => setTimeout(resolve, 400));
  expect(api.captureThumbnail).toHaveBeenCalledWith('monitor', 1, 320);
  expect(document.querySelector('.source img')).not.toBeNull();
});

it('the Windows tab lists windows with their app, and starting shares exactly the chosen source', async () => {
  const start = vi.spyOn(api, 'startScreenShare').mockResolvedValue();
  const closed = vi.fn();
  mount(() => <SharePickerModal open onClose={closed} />);
  await flush();
  button('Windows').click();
  await flush();
  const tile = document.querySelector('[role="option"]') as HTMLElement;
  expect(tile.textContent).toContain('Rocket League');
  expect(tile.textContent).toContain('RocketLeague.exe');
  tile.click();
  await flush();
  button('Start sharing').click();
  await flush();
  await flush();
  expect(start).toHaveBeenCalledWith('window', 9, false, 'system', 30);
  expect(closed).toHaveBeenCalled();
});

it('unsupported Windows warns that viewers hear themselves, and sound is passed on', async () => {
  vi.spyOn(api, 'processLoopbackSupported').mockResolvedValue(false);
  const start = vi.spyOn(api, 'startScreenShare').mockResolvedValue();
  mount(() => <SharePickerModal open onClose={() => {}} />);
  await flush();
  expect(shareAudioToggle().getAttribute('aria-checked')).toBe('false');
  expect(document.body.textContent).not.toContain('hear themselves');
  shareAudioToggle().click();
  await flush();
  expect(localStorage.getItem('finchspeak.shareAudio')).toBe('1');
  expect(document.body.textContent).toContain('This version of Windows cannot leave FinchSpeak out of the sound: people in the room will hear themselves with a delay. Update to Windows 10 2004 or newer.');
  button('Start sharing').click();
  await flush();
  await flush();
  expect(start).toHaveBeenCalledWith('monitor', 1, true, 'system', 30);
});

it('a capture that fails stays in the dialog with the reason, and Cancel does not share anything', async () => {
  const start = vi.spyOn(api, 'startScreenShare').mockRejectedValue('Windows refused to capture this screen');
  const closed = vi.fn();
  mount(() => <SharePickerModal open onClose={closed} />);
  await flush();
  button('Start sharing').click();
  await flush();
  await flush();
  expect(document.querySelector('[role="alert"]')?.textContent).toContain('Windows refused to capture this screen');
  expect(closed).not.toHaveBeenCalled();
  start.mockClear();
  button('Cancel').click();
  expect(closed).toHaveBeenCalled();
  expect(start).not.toHaveBeenCalled();
});

it('says so when there is nothing to share', async () => {
  (api.listScreenSources as ReturnType<typeof vi.fn>).mockResolvedValue({ monitors: [], windows: [] });
  mount(() => <SharePickerModal open onClose={() => {}} />);
  await flush();
  await flush();
  expect(document.body.textContent).toContain('No screens found');
  expect(button('Start sharing').disabled).toBe(true);
});

const fpsButton = (value: string) =>
  Array.from(document.querySelectorAll<HTMLButtonElement>('[aria-label="Frame rate"] button')).find((b) => b.textContent === value) as HTMLButtonElement;

it('shares at 30 FPS by default, and a chosen 60 is sent and remembered', async () => {
  const start = vi.spyOn(api, 'startScreenShare').mockResolvedValue();
  mount(() => <SharePickerModal open onClose={() => {}} />);
  await flush();
  button('Start sharing').click();
  await flush();
  await flush();
  expect(start).toHaveBeenLastCalledWith('monitor', 1, false, 'system', 30);
  fpsButton('60').click();
  await flush();
  button('Start sharing').click();
  await flush();
  await flush();
  expect(start).toHaveBeenLastCalledWith('monitor', 1, false, 'system', 60);
  expect(localStorage.getItem('finchspeak.shareFps')).toBe('60');
});

it('a saved 60 is preselected, and the 1080p note is shown only for 60', async () => {
  localStorage.setItem('finchspeak.shareFps', '60');
  mount(() => <SharePickerModal open onClose={() => {}} />);
  await flush();
  expect(fpsButton('60').getAttribute('aria-pressed')).toBe('true');
  expect(fpsButton('30').getAttribute('aria-pressed')).toBe('false');
  expect(document.body.textContent).toContain('The picture is limited to 1080p.');
  fpsButton('30').click();
  await flush();
  expect(fpsButton('30').getAttribute('aria-pressed')).toBe('true');
  expect(document.body.textContent).not.toContain('The picture is limited to 1080p.');
});

it('the sharer plate shows the set frame rate, then the measured rate and bitrate', async () => {
  setSharingInfo({ label: 'DISPLAY1', audio: false, fps: 60 });
  mount(() => <SharerPill />);
  expect(document.body.textContent).toContain('Without sound · 60 fps');
  setShareStats({ sent_fps: 58.4, captured_fps: 60, encode_ms: 9, bitrate_kbps: 6900, target_fps: 60 });
  await flush();
  expect(document.body.textContent).toContain('Without sound · 58 fps · ' + formatBitrate(6900));
});

// ---- sharer plate (E2)

it('while you share, a plate says what and offers to stop', async () => {
  const stop = vi.spyOn(api, 'stopScreenShare').mockResolvedValue();
  setIsSharing(true);
  setSharingInfo({ label: 'DISPLAY1', audio: true, fps: 30 });
  const host = mount(() => <SharerPill />);
  expect(host.textContent).toContain('You are sharing DISPLAY1');
  expect(host.textContent).toContain('With system sound');
  button('Stop sharing').click();
  await flush();
  expect(stop).toHaveBeenCalled();
});

// ---- viewer (E3/E4)

it('without H.264 support the viewer says why, offers the details, and never shows a black box', async () => {
  vi.stubGlobal('VideoDecoder', undefined);
  setWatching({ user_id: 'carol', peer_id: 5 });
  const host = mount(() => <ViewerPanel />);
  await flush();
  await new Promise((resolve) => setTimeout(resolve, 600));
  expect(host.textContent).toContain('This computer cannot play the video');
  expect(host.textContent).toContain('Copy details');
});

it('a viewer that waits for its first picture says so and asks the sharer for a key frame', async () => {
  class FakeDecoder {
    static isConfigSupported = async () => ({ supported: true });
    state = 'configured';
    configure() {}
    close() { this.state = 'closed'; }
  }
  vi.stubGlobal('VideoDecoder', FakeDecoder);
  setWatching({ user_id: 'carol', peer_id: 5 });
  const host = mount(() => <ViewerPanel />);
  await flush();
  expect(host.textContent).toContain('Waiting for the first picture…');
  expect(host.textContent).toContain('Carol');
  await new Promise((resolve) => setTimeout(resolve, 1600));
  expect(core.invoke).toHaveBeenCalledWith('request_idr_keyframe', { targetPeerId: 5 });
  expect(core.invoke).toHaveBeenCalledWith('register_video_channel', expect.anything());
});

it('Stop watching leaves the share', async () => {
  vi.stubGlobal('VideoDecoder', undefined);
  const unsubscribe = vi.spyOn(api, 'unsubscribeScreenShare').mockResolvedValue();
  setWatching({ user_id: 'carol', peer_id: 5 });
  mount(() => <ViewerPanel />);
  await flush();
  button('Stop watching').click();
  await flush();
  expect(unsubscribe).toHaveBeenCalledWith('carol');
});

it('an ended share says why for each reason, and Back to the room closes it', () => {
  const cases = [
    ['stopped', 'Carol stopped sharing'],
    ['left', 'Carol left the room'],
    ['disconnected', 'Carol lost the connection'],
    ['room-closed', 'The voice room was closed'],
  ] as const;
  for (const [reason, headline] of cases) {
    setEndedShare({ user_id: 'carol', peer_id: 5, name: 'Carol', reason });
    const host = mount(() => <EndedPanel />);
    expect(host.textContent).toContain(headline);
    button('Back to the room').click();
    expect(endedShare()).toBeNull();
    dispose?.();
    document.body.innerHTML = '';
  }
});

it('an ended screen closes itself after ten seconds', () => {
  vi.useFakeTimers();
  setEndedShare({ user_id: 'carol', peer_id: 5, name: 'Carol', reason: 'stopped' });
  mount(() => <EndedPanel />);
  vi.advanceTimersByTime(9_900);
  expect(endedShare()).not.toBeNull();
  vi.advanceTimersByTime(200);
  expect(endedShare()).toBeNull();
});

it('shows a monitor as DISPLAY1, not as a device path', () => {
  expect(monitorName(String.raw`\\.\DISPLAY1`)).toBe('DISPLAY1');
  expect(monitorName('Built-in display')).toBe('Built-in display');
});

it('process loopback shares sound by default without an amber warning', async () => {
  vi.spyOn(api, 'processLoopbackSupported').mockResolvedValue(true);
  const start = vi.spyOn(api, 'startScreenShare').mockResolvedValue();
  mount(() => <SharePickerModal open onClose={() => {}} />);
  await flush();
  expect(shareAudioToggle().getAttribute('aria-checked')).toBe('true');
  expect(localStorage.getItem('finchspeak.shareAudio')).toBeNull();
  expect(document.body.textContent).toContain('Everything your computer plays goes out with the picture, except voices from FinchSpeak.');
  expect(document.querySelector('.auth-banner.amber')).toBeNull();
  fpsButton('60').click();
  button('Start sharing').click();
  await flush();
  expect(start).toHaveBeenCalledWith('monitor', 1, true, 'system', 60);
});

it('stores an explicit choice to turn off the default audio', async () => {
  vi.spyOn(api, 'processLoopbackSupported').mockResolvedValue(true);
  mount(() => <SharePickerModal open onClose={() => {}} />);
  await flush();
  shareAudioToggle().click();
  expect(shareAudioToggle().getAttribute('aria-checked')).toBe('false');
  expect(localStorage.getItem('finchspeak.shareAudio')).toBe('0');
});

it.each([
  { supported: true, stored: '0', checked: 'false' },
  { supported: false, stored: '1', checked: 'true' },
])('reads stored audio choice $stored when reopening (loopback support: $supported)', async ({ supported, stored, checked }) => {
  vi.spyOn(api, 'processLoopbackSupported').mockResolvedValue(supported);
  const [open, setOpen] = createSignal(true);
  mount(() => <SharePickerModal open={open()} onClose={() => setOpen(false)} />);
  await flush();
  button('Cancel').click();
  localStorage.setItem('finchspeak.shareAudio', stored);
  setOpen(true);
  await flush();
  expect(shareAudioToggle().getAttribute('aria-checked')).toBe(checked);
});

it('ignores a loopback check from an earlier opening', async () => {
  let resolveFirstCheck!: (supported: boolean) => void;
  const firstCheck = new Promise<boolean>((resolve) => { resolveFirstCheck = resolve; });
  vi.spyOn(api, 'processLoopbackSupported').mockReturnValueOnce(firstCheck).mockResolvedValue(false);
  const [open, setOpen] = createSignal(true);
  mount(() => <SharePickerModal open={open()} onClose={() => setOpen(false)} />);
  await flush();
  expect(button('Start sharing').disabled).toBe(true);
  button('Cancel').click();
  setOpen(true);
  await flush();
  expect(button('Start sharing').disabled).toBe(false);
  resolveFirstCheck(true);
  await flush();
  expect(shareAudioToggle().getAttribute('aria-checked')).toBe('false');
  expect(document.body.textContent).toContain('Everything your speakers play goes out with the picture.');
});

it('warns about runtime device fallback while the share is active', () => {
  setSharingInfo({ label: 'DISPLAY1', audio: true, fps: 30 });
  setScreenAudioSource('device');
  mount(() => <SharerPill />);
  expect(document.querySelector('.auth-banner.amber')?.textContent).toContain('people in the room will hear themselves with a delay');
  setScreenAudioSource('all-except-self');
  expect(document.querySelector('.auth-banner.amber')).toBeNull();
});

it('offers window sound only for a supported window and sends the selected scope', async () => {
  vi.spyOn(api, 'processLoopbackSupported').mockResolvedValue(true);
  const start = vi.spyOn(api, 'startScreenShare').mockResolvedValue();
  mount(() => <SharePickerModal open onClose={() => {}} />);
  await flush();
  expect(button('Only this window')).toBeUndefined();
  button('Windows').click();
  await flush();
  (document.querySelector('[role="option"]') as HTMLElement).click();
  await flush();
  expect(button('Whole computer').getAttribute('aria-pressed')).toBe('true');
  button('Only this window').click();
  await flush();
  expect(button('Only this window').getAttribute('aria-pressed')).toBe('true');
  expect(document.body.textContent).not.toContain('Everything your computer plays goes out with the picture');
  button('Start sharing').click();
  await flush();
  expect(start).toHaveBeenCalledWith('window', 9, true, 'window', 30);
});

it('does not offer window scope on unsupported Windows', async () => {
  vi.spyOn(api, 'processLoopbackSupported').mockResolvedValue(false);
  mount(() => <SharePickerModal open onClose={() => {}} />);
  await flush();
  shareAudioToggle().click();
  button('Windows').click();
  await flush();
  (document.querySelector('[role="option"]') as HTMLElement).click();
  await flush();
  expect(button('Only this window')).toBeUndefined();
});
