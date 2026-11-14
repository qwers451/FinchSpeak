import { render } from 'solid-js/web';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { DEFAULT_PREFS, setPrefs } from '../../stores/settingsStore';
import type { AttachmentInfo, Channel, VoiceNote } from '../../types';
import { api } from '../../services/api';
import { resetChatState, setActiveChannel } from '../../stores/chatStore';
import { Attachments } from './AttachmentView';
import { beginRecording, cancelRecording, clock, finishRecording, isVoiceNote, MIN_VOICE_MS, nextSpeed, shouldSendVoiceNote, speedLabel, voiceMessagesAvailable, VoiceNotePlayer, VoiceRecordButton, waveformBars } from './VoiceNote';

vi.mock('../../services/api', () => ({
  api: {
    downloadFile: vi.fn(() => new Promise(() => {})),
    downloadThumb: vi.fn(() => new Promise(() => {})),
    voiceNoteStart: vi.fn(() => Promise.resolve()),
    voiceNoteStop: vi.fn(() => Promise.resolve({ bytes: [1, 2], duration_ms: 3000, waveform: new Array(64).fill(0) })),
    voiceNoteCancel: vi.fn(() => Promise.resolve()),
    uploadVoiceNote: vi.fn(() => Promise.resolve({ id: 'a1' })),
    sendTextMessage: vi.fn(() => Promise.resolve()),
  },
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

const originalChannel: Channel = { id: 'text-a', short_id: 1, name: 'A', channel_type: 'text', position: 0, bitrate_kbps: 64 };
const otherChannel: Channel = { ...originalChannel, id: 'text-b', short_id: 2, name: 'B' };
const note: VoiceNote = { bytes: [1, 2], duration_ms: 3000, waveform: new Array(64).fill(0) };
const flush = async () => { for (let i = 0; i < 10; i++) await Promise.resolve(); };
const lockRecording = async () => {
  const host = mount(() => <VoiceRecordButton disabled={false} />);
  host.querySelector<HTMLButtonElement>('.voice-mic')!.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true, clientX: 200 }));
  window.dispatchEvent(new MouseEvent('pointerup', { bubbles: true, clientX: 200 }));
  await flush();
  return host;
};

beforeEach(() => {
  setActiveChannel(originalChannel);
  setPrefs({ ...DEFAULT_PREFS, appearance: { ...DEFAULT_PREFS.appearance, language: 'en' } });
});

afterEach(async () => {
  dispose?.();
  dispose = undefined;
  document.body.innerHTML = '';
  await cancelRecording();
  resetChatState();
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

it('holding the mic and releasing it sends the recording', async () => {
  vi.useFakeTimers();
  try {
    const host = mount(() => <VoiceRecordButton disabled={false} />);
    host.querySelector<HTMLButtonElement>('.voice-mic')!.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true, clientX: 200 }));
    await vi.advanceTimersByTimeAsync(600);
    window.dispatchEvent(new MouseEvent('pointerup', { bubbles: true, clientX: 200 }));
    await vi.advanceTimersByTimeAsync(10);
    expect(api.voiceNoteStop).toHaveBeenCalledTimes(1);
    expect(api.uploadVoiceNote).toHaveBeenCalledTimes(1);
    expect(api.sendTextMessage).toHaveBeenCalledWith('text-a', '', ['a1'], expect.any(String), undefined);
  } finally {
    await cancelRecording();
    vi.useRealTimers();
  }
});

it('leaving the chat while the mic is held cancels the recording instead of sending it', async () => {
  vi.useFakeTimers();
  try {
    const host = mount(() => <VoiceRecordButton disabled={false} />);
    host.querySelector<HTMLButtonElement>('.voice-mic')!.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true, clientX: 200 }));
    await vi.advanceTimersByTimeAsync(100);
    dispose?.();
    dispose = undefined;
    await vi.advanceTimersByTimeAsync(10);
    expect(api.voiceNoteCancel).toHaveBeenCalledTimes(1);
    window.dispatchEvent(new MouseEvent('pointerup', { bubbles: true, clientX: 200 }));
    await vi.advanceTimersByTimeAsync(600);
    expect(api.voiceNoteStop).not.toHaveBeenCalled();
    expect(api.uploadVoiceNote).not.toHaveBeenCalled();
  } finally {
    await cancelRecording();
    vi.useRealTimers();
  }
});


it('unmounting a tap-locked recording cancels it instead of sending later', async () => {
  await lockRecording();
  dispose?.();
  dispose = undefined;
  await flush();
  expect(api.voiceNoteCancel).toHaveBeenCalledTimes(1);
  await finishRecording();
  expect(api.voiceNoteStop).not.toHaveBeenCalled();
  expect(api.sendTextMessage).not.toHaveBeenCalled();
});

it('switching text channels cancels a locked recording without unmounting the composer', async () => {
  const host = await lockRecording();
  setActiveChannel(otherChannel);
  await flush();
  expect(api.voiceNoteCancel).toHaveBeenCalledTimes(1);
  expect(host.querySelector('.voice-mic')).not.toBeNull();
  await finishRecording();
  expect(api.sendTextMessage).not.toHaveBeenCalled();
});

it('switching channels while stopping discards the result before uploading', async () => {
  let resolveStop!: (value: VoiceNote) => void;
  vi.mocked(api.voiceNoteStop).mockImplementationOnce(() => new Promise((resolve) => { resolveStop = resolve; }));
  await lockRecording();
  const sending = finishRecording();
  await flush();
  setActiveChannel(otherChannel);
  resolveStop(note);
  await sending;
  expect(api.uploadVoiceNote).not.toHaveBeenCalled();
  expect(api.sendTextMessage).not.toHaveBeenCalled();
});

it('switching channels during upload never sends the old note to the new channel', async () => {
  let resolveUpload!: (value: AttachmentInfo) => void;
  vi.mocked(api.uploadVoiceNote).mockImplementationOnce(() => new Promise((resolve) => { resolveUpload = resolve; }));
  await lockRecording();
  const sending = finishRecording();
  await flush();
  expect(api.uploadVoiceNote).toHaveBeenCalledTimes(1);
  setActiveChannel(otherChannel);
  resolveUpload(voice);
  await sending;
  expect(api.sendTextMessage).not.toHaveBeenCalled();
});

it('unmounting during upload prevents the completed upload from sending', async () => {
  let resolveUpload!: (value: AttachmentInfo) => void;
  vi.mocked(api.uploadVoiceNote).mockImplementationOnce(() => new Promise((resolve) => { resolveUpload = resolve; }));
  await lockRecording();
  const sending = finishRecording();
  await flush();
  dispose?.();
  dispose = undefined;
  resolveUpload(voice);
  await sending;
  expect(api.sendTextMessage).not.toHaveBeenCalled();
});

it.each(['unmount', 'channel switch'] as const)('cancels a pending native start after %s even after release locked it', async (action) => {
  let resolveStart!: () => void;
  vi.mocked(api.voiceNoteStart).mockImplementationOnce(() => new Promise((resolve) => { resolveStart = resolve; }));
  await lockRecording();
  if (action === 'unmount') {
    dispose?.();
    dispose = undefined;
  } else setActiveChannel(otherChannel);
  resolveStart();
  await flush();
  expect(api.voiceNoteCancel).toHaveBeenCalledTimes(1);
  await finishRecording();
  expect(api.voiceNoteStop).not.toHaveBeenCalled();
  expect(api.uploadVoiceNote).not.toHaveBeenCalled();
  expect(api.sendTextMessage).not.toHaveBeenCalled();
});


it('a locked recording can still be sent in its original channel', async () => {
  const host = await lockRecording();
  host.querySelector<HTMLButtonElement>('[aria-label="Send voice message"]')!.click();
  await flush();
  expect(api.voiceNoteCancel).not.toHaveBeenCalled();
  expect(api.voiceNoteStop).toHaveBeenCalledTimes(1);
  expect(api.sendTextMessage).toHaveBeenCalledWith('text-a', '', ['a1'], expect.any(String), undefined);
});


it('a cancelled native stop must finish before another recording can start', async () => {
  let resolveStop!: (value: VoiceNote) => void;
  vi.mocked(api.voiceNoteStop).mockImplementationOnce(() => new Promise((resolve) => { resolveStop = resolve; }));
  await lockRecording();
  const sending = finishRecording();
  await flush();
  await cancelRecording();
  try {
    await beginRecording('locked');
    expect(api.voiceNoteStart).toHaveBeenCalledTimes(1);
  } finally {
    resolveStop(note);
    await sending;
  }
  await beginRecording('locked');
  expect(api.voiceNoteStart).toHaveBeenCalledTimes(2);
  expect(api.uploadVoiceNote).not.toHaveBeenCalled();
});
