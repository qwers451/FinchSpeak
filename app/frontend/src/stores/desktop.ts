import { createEffect, createRoot } from 'solid-js';
import { api } from '../services/api';
import type { Message } from '../types';
import { currentUser } from './authStore';
import { activeChannel, channels } from './chatStore';
import { isChannelMuted, prefs } from './settingsStore';
import { isSharing } from './screenState';
import { activeVoiceChannel, isMuted } from './voiceStore';
import { t } from '../i18n';

// Desktop behaviour: notifications, tones, the tray icon. Everything here is optional: if the
// platform refuses, the app keeps working without it.

// ---- tones (made in code: no sound files to ship) ---------------------------------------------

export type Tone = 'mention' | 'join' | 'leave';

/** Notes (Hz) and length (ms) of each tone. */
export const TONES: Record<Tone, { notes: number[]; noteMs: number }> = {
  mention: { notes: [988, 1319], noteMs: 110 },
  join: { notes: [660, 880], noteMs: 90 },
  leave: { notes: [880, 660], noteMs: 90 },
};

let context: AudioContext | null = null;

/** Play a tone at the chosen volume; silently does nothing when sounds are off or unavailable. */
export function playTone(tone: Tone, volume = prefs().notifications.soundVolume) {
  if (!prefs().notifications.sounds || volume <= 0) return;
  try {
    const Audio = window.AudioContext ?? (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
    if (!Audio) return;
    context ??= new Audio();
    const { notes, noteMs } = TONES[tone];
    const start = context.currentTime + 0.01;
    notes.forEach((frequency, index) => {
      const oscillator = context!.createOscillator();
      const gain = context!.createGain();
      const at = start + (index * noteMs) / 1000;
      oscillator.type = 'sine';
      oscillator.frequency.value = frequency;
      gain.gain.setValueAtTime(0, at);
      gain.gain.linearRampToValueAtTime(Math.min(1, volume) * 0.3, at + 0.01);
      gain.gain.exponentialRampToValueAtTime(0.0001, at + noteMs / 1000);
      oscillator.connect(gain).connect(context!.destination);
      oscillator.start(at);
      oscillator.stop(at + noteMs / 1000 + 0.02);
    });
  } catch { /* no sound is not an error */ }
}

// ---- notifications ----------------------------------------------------------------------------

const isTauri = () => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/** Show an operating-system notification (only meaningful while the window is in the background). */
export async function notifyDesktop(title: string, body: string) {
  if (!isTauri()) return;
  try {
    const { isPermissionGranted, requestPermission, sendNotification } = await import('@tauri-apps/plugin-notification');
    let granted = await isPermissionGranted();
    if (!granted) granted = (await requestPermission()) === 'granted';
    if (granted) sendNotification({ title, body: body.slice(0, 200) });
  } catch { /* notifications are optional */ }
}

async function flashTaskbar() {
  if (!isTauri()) return;
  try {
    const { getCurrentWindow, UserAttentionType } = await import('@tauri-apps/api/window');
    await getCurrentWindow().requestUserAttention(UserAttentionType.Informational);
  } catch { /* optional */ }
}

/** What to do about a message that just arrived (not ours): returns what was done, for tests. */
export function reactToMessage(
  message: Pick<Message, 'channel_id' | 'author_id' | 'author_name' | 'content' | 'mentions'>,
  mentionedMe: boolean,
  focused = document.hasFocus(),
): 'none' | 'notified' {
  const user = currentUser();
  const settings = prefs().notifications;
  if (!user || message.author_id === user.id) return 'none';
  if (isChannelMuted(message.channel_id)) return 'none';
  const viewing = focused && activeChannel()?.id === message.channel_id;
  if (viewing) return 'none';
  const wanted = mentionedMe ? settings.mentions : settings.allMessages;
  if (!wanted) return 'none';
  const channel = channels().find((candidate) => candidate.id === message.channel_id)?.name ?? t('a channel');
  if (!focused) {
    void notifyDesktop(mentionedMe ? t('{name} mentioned you in #{channel}', { name: message.author_name, channel }) : t('{name} in #{channel}', { name: message.author_name, channel }), message.content);
    if (settings.flash) void flashTaskbar();
  }
  if (mentionedMe) playTone('mention');
  return 'notified';
}

/** Somebody came into or left the voice room I am in. */
export function reactToVoiceMovement(kind: 'join' | 'leave', name: string, userId: string, focused = document.hasFocus()) {
  const settings = prefs().notifications;
  if (!settings.voiceJoin || userId === currentUser()?.id) return;
  playTone(kind);
  if (kind === 'join' && !focused) void notifyDesktop(t('{name} joined {room}', { name, room: activeVoiceChannel()?.name ?? t('your voice room') }), '');
}

// ---- tray and window --------------------------------------------------------------------------

export type TrayState = 'idle' | 'voice' | 'muted' | 'live';

/** The tray icon follows the call: idle, in a room, muted, or live (sharing). */
export function trayStateNow(): TrayState {
  if (isSharing()) return 'live';
  if (!activeVoiceChannel()) return 'idle';
  return isMuted() ? 'muted' : 'voice';
}

let started = false;
/** Keep the tray icon and the close behaviour in step with the app (once, for the life of the app). */
export function startDesktopSync() {
  if (started) return;
  started = true;
  createRoot(() => {
    createEffect(() => {
      const state = trayStateNow();
      void api.setTrayState(state, prefs().appearance.language).catch(() => {});
    });
    createEffect(() => {
      void api.setCloseToTray(prefs().appearance.closeToTray).catch(() => {});
    });
  });
}
