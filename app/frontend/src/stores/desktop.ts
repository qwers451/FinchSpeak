import { createEffect, createRoot } from 'solid-js';
import { api } from '../services/api';
import type { Message } from '../types';
import { currentUser } from './authStore';
import { activeChannel, channels, selectChannel } from './chatStore';
import { myStatus } from './sessionStore';
import { unread } from './activityStore';
import { isChannelMuted, prefs } from './settingsStore';
import { isSharing } from './screenState';
import { activeVoiceChannel, isMuted } from './voiceStore';
import { t } from '../i18n';
import { playTone } from './tones';

export { TONES, playTone, type Tone } from './tones';

// Desktop behaviour: notifications, tones, the tray icon. Everything here is optional: if the
// platform refuses, the app keeps working without it.

// ---- notifications ----------------------------------------------------------------------------

/** The channel of the last notification we showed; the platform does not report clicks, so a focus soon after stands in for one. */
let lastNotified: { channelId: string; at: number } | null = null;
/** A focus within this long after a notification opens its channel. */
export const NOTIFICATION_JUMP_MS = 20_000;

const isTauri = () => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/** Show an operating-system notification (only meaningful while the window is in the background). */
export async function notifyDesktop(title: string, body: string, channelId?: string) {
  if (!isTauri()) return;
  try {
    const { isPermissionGranted, requestPermission, sendNotification } = await import('@tauri-apps/plugin-notification');
    let granted = await isPermissionGranted();
    if (!granted) granted = (await requestPermission()) === 'granted';
    if (granted) {
      sendNotification({ title, body: body.slice(0, 200) });
      if (channelId) lastNotified = { channelId, at: Date.now() };
    }
  } catch { /* notifications are optional */ }
}

async function flashTaskbar() {
  if (!isTauri()) return;
  try {
    const { getCurrentWindow, UserAttentionType } = await import('@tauri-apps/api/window');
    await getCurrentWindow().requestUserAttention(UserAttentionType.Informational);
  } catch { /* optional */ }
}

/** True while my status is "Do not disturb": no OS notifications, taskbar flash or mention tone. */
export function isDoNotDisturb(): boolean {
  return currentUser() !== null && myStatus() === 'donotdisturb';
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
  if (isDoNotDisturb()) return 'none';
  if (isChannelMuted(message.channel_id)) return 'none';
  const viewing = focused && activeChannel()?.id === message.channel_id;
  if (viewing) return 'none';
  const wanted = mentionedMe ? settings.mentions : settings.allMessages;
  if (!wanted) return 'none';
  const channel = channels().find((candidate) => candidate.id === message.channel_id)?.name ?? t('a channel');
  if (!focused) {
    void notifyDesktop(mentionedMe ? t('{name} mentioned you in #{channel}', { name: message.author_name, channel }) : t('{name} in #{channel}', { name: message.author_name, channel }), message.content, message.channel_id);
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
  if (kind === 'join' && !focused && !isDoNotDisturb()) void notifyDesktop(t('{name} joined {room}', { name, room: activeVoiceChannel()?.name ?? t('your voice room') }), '');
}

// ---- tray and window --------------------------------------------------------------------------

export type TrayState = 'idle' | 'voice' | 'muted' | 'live';

/** The tray icon follows the call: idle, in a room, muted, or live (sharing). */
export function trayStateNow(): TrayState {
  if (isSharing()) return 'live';
  if (!activeVoiceChannel()) return 'idle';
  return isMuted() ? 'muted' : 'voice';
}

/** Mentions in channels that are not muted (the taskbar badge). */
export function mentionCount(): number {
  return Object.entries(unread())
    .filter(([channelId]) => !isChannelMuted(channelId))
    .reduce((sum, [, entry]) => sum + (entry.mentions ?? (entry.mention ? 1 : 0)), 0);
}

/** The window came to the front: if that was soon after a notification, open its channel. */
function jumpToNotifiedChannel() {
  const last = lastNotified;
  if (!last) return;
  lastNotified = null;
  if (Date.now() - last.at >= NOTIFICATION_JUMP_MS) return;
  const channel = channels().find((candidate) => candidate.id === last.channelId);
  if (channel) void selectChannel(channel);
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
      void api.setAttention(mentionCount(), prefs().appearance.language).catch(() => {});
    });
    window.addEventListener('focus', jumpToNotifiedChannel);
  });
}
