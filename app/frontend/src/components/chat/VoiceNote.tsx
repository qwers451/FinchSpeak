import { createEffect, createResource, createSignal, For, on, onCleanup, onMount, Show } from 'solid-js';
import { listen } from '@tauri-apps/api/event';
import { api } from '../../services/api';
import { activeChannel, sendMessage } from '../../stores/chatStore';
import { setAppError } from '../../stores/notificationStore';
import type { AttachmentInfo } from '../../types';
import { Button, Icon } from '../../ui';
import { t } from '../../i18n';
import { describeError } from '../../i18n/errors';

/** Values in a voice message's waveform (see the server and `voice_note.rs`). */
export const WAVEFORM_BARS = 64;
/** Shorter recordings are dropped: a stray tap of the microphone button sends nothing. */
export const MIN_VOICE_MS = 500;
/** A press shorter than this locks the recording until the next tap; a longer one sends on release. */
const TAP_MS = 400;
/** Swiping this far to the left while holding cancels the recording. */
const SWIPE_CANCEL_PX = 80;
/** What the message reads as in the chat; the audio itself is the attachment. */
/** Voice messages carry no text: the attachment is the message. */
export const VOICE_NOTE_TEXT = '';
export const PLAYBACK_SPEEDS = [1, 1.5, 2];

/** Whether the voice message controls are shown: the app itself, and a server that has the feature. */
export function voiceMessagesAvailable(features: string[], inApp: boolean): boolean {
  return inApp && features.includes('voice-messages');
}

export const inApp = () => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

export const shouldSendVoiceNote = (durationMs: number) => durationMs >= MIN_VOICE_MS;

/** The next playback speed: 1×, 1,5×, 2×, then 1× again. */
export function nextSpeed(speed: number): number {
  const index = PLAYBACK_SPEEDS.findIndex((candidate) => candidate === speed);
  return PLAYBACK_SPEEDS[(index + 1) % PLAYBACK_SPEEDS.length];
}

export const speedLabel = (speed: number) => `${speed === 1.5 ? '1,5' : String(speed)}×`;

/** `m:ss` for a length in milliseconds. */
export function clock(ms: number): string {
  const seconds = Math.max(0, Math.round(ms / 1000));
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, '0')}`;
}

/** The 64 bars of a waveform as fractions of full height; a missing waveform is flat. */
export function waveformBars(values: number[] | null | undefined): number[] {
  return Array.from({ length: WAVEFORM_BARS }, (_, index) => Math.min(1, Math.max(0, (values?.[index] ?? 0) / 255)));
}

/** A voice message is an Ogg file that carries a length; any other audio stays a file. */
export const isVoiceNote = (attachment: AttachmentInfo) =>
  attachment.content_type === 'audio/ogg' && attachment.duration_ms != null;

// --- playing ----------------------------------------------------------------------------------

const audioUrls = new Map<string, Promise<string>>();

/** A `blob:` URL of the message audio, fetched once with our login. */
function audioUrl(attachment: AttachmentInfo): Promise<string> {
  const known = audioUrls.get(attachment.id);
  if (known) return known;
  const loading = api.downloadFile(attachment.id).then((bytes) => URL.createObjectURL(new Blob([bytes], { type: 'audio/ogg' })));
  loading.catch(() => audioUrls.delete(attachment.id));
  audioUrls.set(attachment.id, loading);
  return loading;
}

/** The player of a voice message: play/pause, the waveform with the played part filled, length and speed. */
export function VoiceNotePlayer(props: { attachment: AttachmentInfo }) {
  const [url] = createResource(() => props.attachment.id, () => audioUrl(props.attachment));
  const [playing, setPlaying] = createSignal(false);
  const [speed, setSpeed] = createSignal(1);
  const [positionMs, setPositionMs] = createSignal(0);
  const [durationMs, setDurationMs] = createSignal(props.attachment.duration_ms ?? 0);
  const bars = () => waveformBars(props.attachment.waveform);
  const progress = () => (durationMs() > 0 ? Math.min(1, positionMs() / durationMs()) : 0);
  let audio: HTMLAudioElement | undefined;

  const toggle = () => {
    if (!audio) return;
    if (audio.paused) void audio.play().catch(() => setPlaying(false));
    else audio.pause();
  };
  const cycleSpeed = () => {
    const next = nextSpeed(speed());
    setSpeed(next);
    if (audio) audio.playbackRate = next;
  };

  return (
    <div class="voice-note" role="group" aria-label={t('Voice message')}>
      <button type="button" class="voice-play" aria-label={playing() ? t('Pause') : t('Play')} disabled={!url()} onClick={toggle}>
        <Show when={playing()} fallback={<Icon name="play" size="sm" />}>
          <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true"><rect x="1" y="0" width="3.5" height="12" rx="1" fill="currentColor" /><rect x="7.5" y="0" width="3.5" height="12" rx="1" fill="currentColor" /></svg>
        </Show>
      </button>
      <div class="voice-wave" aria-hidden="true">
        <For each={bars()}>
          {(value, index) => (
            <span class={`voice-bar${(index() + 1) / WAVEFORM_BARS <= progress() ? ' played' : ''}`} style={{ height: `${Math.max(12, value * 100)}%` }} />
          )}
        </For>
      </div>
      <span class="voice-time tnum t3 fs12">{clock(positionMs() > 0 ? positionMs() : durationMs())}</span>
      <button type="button" class="voice-speed" aria-label={t('Playback speed')} onClick={cycleSpeed}>{speedLabel(speed())}</button>
      <audio
        ref={audio}
        src={url() ?? undefined}
        preload="metadata"
        hidden
        onLoadedMetadata={(event) => {
          const seconds = event.currentTarget.duration;
          if (Number.isFinite(seconds)) setDurationMs(Math.round(seconds * 1000));
        }}
        onTimeUpdate={(event) => setPositionMs(event.currentTarget.currentTime * 1000)}
        onPlay={() => setPlaying(true)}
        onPause={() => setPlaying(false)}
        onEnded={() => { setPlaying(false); setPositionMs(0); }}
      />
    </div>
  );
}

// --- recording --------------------------------------------------------------------------------

type Phase = 'idle' | 'holding' | 'locked' | 'sending';

const [phase, setPhase] = createSignal<Phase>('idle');
const [level, setLevel] = createSignal(0);
const [startedAt, setStartedAt] = createSignal(0);
interface RecordingSession {
  channelId: string;
  start: Promise<void>;
  started: boolean;
  cancelled: boolean;
  stopping: boolean;
  stopped: boolean;
  cancelling?: Promise<void>;
}
let recording: RecordingSession | null = null;
let listening = false;

export const isRecording = () => phase() !== 'idle';

const isCurrentRecording = (session: RecordingSession) =>
  recording === session && !session.cancelled && activeChannel()?.id === session.channelId;

function clearRecording(session: RecordingSession) {
  if (recording !== session) return;
  recording = null;
  setPhase('idle');
}

export async function beginRecording(mode: 'holding' | 'locked') {
  const channel = activeChannel();
  if (recording || !channel || channel.channel_type !== 'text') return;
  const session: RecordingSession = {
    channelId: channel.id, start: Promise.resolve(), started: false, cancelled: false, stopping: false, stopped: false,
  };
  recording = session;
  setPhase(mode);
  setLevel(0);
  session.start = api.voiceNoteStart().then(
    () => {
      session.started = true;
      if (isCurrentRecording(session)) setStartedAt(Date.now());
    },
    (err) => {
      if (isCurrentRecording(session)) setAppError(t('Recording failed: {error}', { error: describeError(err) }));
      clearRecording(session);
    },
  );
  await session.start;
}

/** Stop and send. A recording shorter than the minimum is dropped without a message. */
export async function finishRecording() {
  const session = recording;
  if (!session) return;
  await session.start;
  if (!isCurrentRecording(session) || session.stopping) return;
  session.stopping = true;
  setPhase('sending');
  try {
    const note = await api.voiceNoteStop();
    session.stopped = true;
    if (!isCurrentRecording(session) || !shouldSendVoiceNote(note.duration_ms)) return;
    const info = await api.uploadVoiceNote(note.bytes, note.duration_ms, note.waveform);
    // sendMessage reads its channel synchronously. Never let a cancelled/stale upload reach it.
    if (isCurrentRecording(session)) await sendMessage(VOICE_NOTE_TEXT, [info.id]);
  } catch (err) {
    if (isCurrentRecording(session)) setAppError(t('Recording failed: {error}', { error: describeError(err) }));
  } finally {
    clearRecording(session);
  }
}

/** Throw the recording away, invalidating any pending start, stop or upload immediately. */
export async function cancelRecording() {
  const session = recording;
  if (!session) return;
  session.cancelled = true;
  if (session.cancelling) return session.cancelling;
  if (session.stopping) {
    // The old native worker detaches the shared tap on exit: reserve the recorder until stop
    // settles. Once only upload remains, a new recording can safely start.
    if (session.stopped) clearRecording(session);
    return;
  }
  session.cancelling = (async () => {
    await session.start;
    try {
      if (session.started) await api.voiceNoteCancel();
    } catch {
      // Nothing is sent either way; the recorder stops on its own when it cannot continue.
    } finally {
      clearRecording(session);
    }
  })();
  return session.cancelling;
}

/** Listens once for the level meter and for the two-minute limit (the recording then stops itself). */
export function listenForVoiceEvents() {
  if (listening || !inApp()) return;
  listening = true;
  void listen<{ level: number }>('voice-note-level', (event) => setLevel(event.payload.level)).catch(() => {});
  void listen('voice-note-limit', () => void finishRecording()).catch(() => {});
}

/** The recording controls of the composer: hold to record, release to send, or tap to lock. */
export function VoiceRecordButton(props: { disabled: boolean }) {
  onMount(() => listenForVoiceEvents());
  let pressedAt = 0;
  let pressedX = 0;
  let holding = false;

  // The button leaves the DOM as soon as recording starts, so the rest of the press is followed on
  // the window: a release or a swipe that lands anywhere still ends it.
  const stopFollowing = () => {
    window.removeEventListener('pointermove', onMove);
    window.removeEventListener('pointerup', onUp);
    window.removeEventListener('pointercancel', onCancel);
  };
  const onMove = (event: PointerEvent) => {
    if (holding && phase() === 'holding' && pressedX - event.clientX > SWIPE_CANCEL_PX) {
      holding = false;
      stopFollowing();
      void cancelRecording();
    }
  };
  const onUp = () => {
    stopFollowing();
    if (!holding) return;
    holding = false;
    if (phase() !== 'holding') return;
    if (Date.now() - pressedAt >= TAP_MS) void finishRecording();
    else setPhase('locked');
  };
  const onCancel = () => {
    stopFollowing();
    holding = false;
    void cancelRecording();
  };
  const cancelOnLeave = () => {
    stopFollowing();
    holding = false;
    void cancelRecording();
  };
  // Text-to-text navigation keeps this component mounted. Its recording still belongs to the
  // original channel, including a locked press and native start/stop/upload in flight.
  createEffect(on(() => activeChannel()?.id, (channelId) => {
    if (recording && recording.channelId !== channelId) cancelOnLeave();
  }, { defer: true }));
  onCleanup(cancelOnLeave);
  const onDown = (event: PointerEvent) => {
    event.preventDefault();
    if (phase() === 'locked') {
      void finishRecording();
      return;
    }
    if (phase() !== 'idle') return;
    holding = true;
    pressedAt = Date.now();
    pressedX = event.clientX;
    window.addEventListener('pointermove', onMove);
    window.addEventListener('pointerup', onUp);
    window.addEventListener('pointercancel', onCancel);
    void beginRecording('holding');
  };

  return (
    <Show when={!isRecording()} fallback={<RecordingBar />}>
      <button
        type="button"
        class="voice-mic"
        aria-label={t('Hold to record a voice message')}
        title={t('Hold to record a voice message')}
        disabled={props.disabled}
        onPointerDown={onDown}
      >
        <Icon name="mic" size="sm" />
      </button>
    </Show>
  );
}

/** What is shown while recording: the timer, the level, the hint, cancel, and send once locked. */
function RecordingBar() {
  const [elapsed, setElapsed] = createSignal(0);
  onMount(() => {
    listenForVoiceEvents();
    const timer = window.setInterval(() => setElapsed(Math.max(0, Date.now() - startedAt())), 200);
    onCleanup(() => window.clearInterval(timer));
  });

  return (
    <div class="voice-recording" role="status" aria-live="polite">
      <span class="voice-dot" aria-hidden="true" />
      <span class="voice-clock tnum fs13 b">{clock(elapsed())}</span>
      <span class="voice-level" aria-hidden="true"><span style={{ transform: `scaleX(${Math.max(0.04, level())})` }} /></span>
      <span class="t3 fs12 trunc">{phase() === 'locked' ? t('Tap again to send') : t('Release to send')}</span>
      <button type="button" class="icon-close" aria-label={t('Cancel recording')} onClick={() => void cancelRecording()}><Icon name="x" size="sm" /></button>
      <Show when={phase() === 'locked'}>
        <Button iconOnly size="sm" variant="primary" icon="send" label={t('Send voice message')} onClick={() => void finishRecording()} />
      </Show>
    </div>
  );
}
