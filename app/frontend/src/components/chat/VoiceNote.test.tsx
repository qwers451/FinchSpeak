import { render } from 'solid-js/web';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { DEFAULT_PREFS, setPrefs } from '../../stores/settingsStore';
import type { AttachmentInfo } from '../../types';
import { Attachments } from './AttachmentView';
import { clock, isVoiceNote, MIN_VOICE_MS, nextSpeed, shouldSendVoiceNote, speedLabel, voiceMessagesAvailable, VoiceNotePlayer, waveformBars } from './VoiceNote';

vi.mock('../../services/api', () => ({
  api: { downloadFile: vi.fn(() => new Promise(() => {})), downloadThumb: vi.fn(() => new Promise(() => {})) },
}));

let dispose: (() => void) | undefined;
const mount = (view: () => unknown) => {
  const host = document.createElement('div');
  document.body.appendChild(host);
  dispose = render(view as () => never, host);
  return host;
};

const waveform = Array.from({ length: 64 }, (_, index) => (index < 32 ? 255 : 0));
const voice: AttachmentInfo = {
  id: 'v1', filename: 'voice-message.ogg', size_bytes: 4096, content_type: 'audio/ogg', url: '/api/files/v1',
  duration_ms: 3000, waveform,
};

beforeEach(() => {
  setPrefs({ ...DEFAULT_PREFS, appearance: { ...DEFAULT_PREFS.appearance, language: 'en' } });
});

afterEach(() => {
  dispose?.();
  dispose = undefined;
  document.body.innerHTML = '';
  vi.clearAllMocks();
});

it('the waveform has 64 bars scaled to 0 to 1, and a missing waveform is flat', () => {
  const bars = waveformBars(waveform);
  expect(bars).toHaveLength(64);
  expect(bars[0]).toBe(1);
  expect(bars[63]).toBe(0);
  expect(waveformBars(null).every((bar) => bar === 0)).toBe(true);
  expect(waveformBars([300, -4])[0]).toBe(1);
});

it('a recording shorter than half a second is not sent', () => {
  expect(MIN_VOICE_MS).toBe(500);
  expect(shouldSendVoiceNote(499)).toBe(false);
  expect(shouldSendVoiceNote(500)).toBe(true);
});

it('playback speed cycles 1, 1.5, 2 and labels use a decimal comma', () => {
  expect(nextSpeed(1)).toBe(1.5);
  expect(nextSpeed(1.5)).toBe(2);
  expect(nextSpeed(2)).toBe(1);
  expect(speedLabel(1)).toBe('1×');
  expect(speedLabel(1.5)).toBe('1,5×');
  expect(speedLabel(2)).toBe('2×');
});

it('length is shown as m:ss', () => {
  expect(clock(3000)).toBe('0:03');
  expect(clock(125_000)).toBe('2:05');
  expect(clock(0)).toBe('0:00');
});

it('voice messages are controls only in the app, on a server with the feature', () => {
  expect(voiceMessagesAvailable(['voice-messages'], true)).toBe(true);
  expect(voiceMessagesAvailable(['voice-messages'], false)).toBe(false);
  expect(voiceMessagesAvailable(['soundboard'], true)).toBe(false);
});

it('only an Ogg file with a length is a voice message', () => {
  expect(isVoiceNote(voice)).toBe(true);
  expect(isVoiceNote({ ...voice, duration_ms: null })).toBe(false);
  expect(isVoiceNote({ ...voice, content_type: 'audio/mpeg' })).toBe(false);
});

it('the player draws the waveform, the length and a speed control', () => {
  const host = mount(() => <VoiceNotePlayer attachment={voice} />);
  expect(host.querySelectorAll('.voice-bar')).toHaveLength(64);
  expect(host.querySelector('.voice-time')?.textContent).toBe('0:03');
  const speed = host.querySelector('.voice-speed') as HTMLButtonElement;
  expect(speed.textContent).toBe('1×');
  speed.click();
  expect(speed.textContent).toBe('1,5×');
  speed.click();
  expect(speed.textContent).toBe('2×');
});

it('a voice message in a chat shows the player, a plain file still a file chip', () => {
  const host = mount(() => <Attachments attachments={[voice, { ...voice, id: 'f2', filename: 'notes.txt', content_type: 'text/plain', duration_ms: null, waveform: null }]} />);
  expect(host.querySelectorAll('.voice-note')).toHaveLength(1);
  expect(host.textContent).toContain('notes.txt');
});
