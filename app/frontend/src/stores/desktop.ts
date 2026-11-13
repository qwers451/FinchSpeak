import { createEffect, createMemo, createRoot, createSignal, onCleanup, untrack, type Accessor } from 'solid-js';
import { listen } from '@tauri-apps/api/event';
import { api } from '../services/api';
import type { FlockCall, Message, NoticeActivation } from '../types';
import { currentUser } from './authStore';
import { activeChannel, channels, selectChannel } from './chatStore';
import { myStatus } from './sessionStore';
import { unread } from './activityStore';
import { isChannelMuted, prefs } from './settingsStore';
import { isSharing } from './screenState';
import { acceptCall, dismissCall, flyToRoom, pendingCallTo } from './flockStore';
import { activeVoiceChannel, allVoiceParticipants, isDeafened, isLocalSpeaking, isMuted } from './voiceStore';
import { t } from '../i18n';
import { playTone } from './tones';
import { playJoinSound } from './joinSound';

export { TONES, playTone, type Tone } from './tones';

// Desktop behaviour: notifications, tones, the tray icon. Everything here is optional: if the
// platform refuses, the app keeps working without it.

// ---- notifications ----------------------------------------------------------------------------

/** A notification to show: the Windows one with picture and buttons, or the plugin's elsewhere. */
export interface NoticeDraft {
  title: string;
  body: string;
  /** Whose picture to show round. */
  avatarUserId?: string;
  target: { kind: 'channel' | 'voice_room'; id: string } | { kind: 'none' };
  /** Buttons; a click on one comes back to `handleNoticeActivated`. */
  actions?: { id: 'fly' | 'stay'; label: string }[];
}

const isTauri = () => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
const isWindows = () => typeof navigator !== 'undefined' && /Windows/.test(navigator.userAgent);

/** The plain notification of the notification plugin: title and text, no picture, no buttons. */
async function notifyWithPlugin(title: string, body: string) {
  try {
    const { isPermissionGranted, requestPermission, sendNotification } = await import('@tauri-apps/plugin-notification');
    let granted = await isPermissionGranted();
    if (!granted) granted = (await requestPermission()) === 'granted';
    if (granted) sendNotification({ title, body: body.slice(0, 200) });
  } catch { /* notifications are optional */ }
}

/** Show an operating-system notification (only meaningful while the window is in the background). */
export async function showNotice(draft: NoticeDraft) {
  if (!isTauri()) return;
  if (isWindows()) {
    try {
      const avatar = draft.avatarUserId ? await api.downloadAvatar(draft.avatarUserId).catch(() => null) : null;
      const actions = draft.actions ?? [];
      await api.notify({
        title: draft.title,
        body: draft.body.slice(0, 200),
        avatar_user_id: draft.avatarUserId ?? null,
        target: draft.target,
        actions,
      }, avatar);
      return;
    } catch { /* the plugin below still tells the person */ }
  }
  await notifyWithPlugin(draft.title, draft.body);
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
  message: Pick<Message, 'channel_id' | 'author_id' | 'author_name' | 'content' | 'mentions' | 'reply_to'>,
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
  const answersMe = message.reply_to?.author_id === user.id;
  const wanted = mentionedMe ? settings.mentions : answersMe ? settings.replies : settings.allMessages;
  if (!wanted) return 'none';
  const channel = channels().find((candidate) => candidate.id === message.channel_id)?.name ?? t('a channel');
  if (!focused) {
    const title = mentionedMe
      ? t('{name} mentioned you in #{channel}', { name: message.author_name, channel })
      : answersMe
        ? t('{name} replied to you in #{channel}', { name: message.author_name, channel })
        : t('{name} in #{channel}', { name: message.author_name, channel });
    void showNotice({ title, body: message.content, avatarUserId: message.author_id, target: { kind: 'channel', id: message.channel_id } });
    if (settings.flash) void flashTaskbar();
  }
  if (mentionedMe) playTone('mention');
  return 'notified';
}

/** Somebody came into or left the voice room I am in. */
export function reactToVoiceMovement(kind: 'join' | 'leave', name: string, userId: string, focused = document.hasFocus()) {
  const settings = prefs().notifications;
  if (!settings.voiceJoin || userId === currentUser()?.id) return;
  if (kind === 'join') void playJoinSound(userId);
  else playTone(kind);
  if (kind !== 'join' || focused || isDoNotDisturb()) return;
  const room = activeVoiceChannel();
  if (room && isChannelMuted(room.id)) return;
  void showNotice({
    title: t('{name} joined {room}', { name, room: room?.name ?? t('your voice room') }),
    body: '',
    avatarUserId: userId,
    target: room ? { kind: 'voice_room', id: room.id } : { kind: 'none' },
  });
}

/** Somebody started sharing a screen in the voice room I am in. */
export function reactToShareStarted(userId: string, focused = document.hasFocus()) {
  const room = activeVoiceChannel();
  if (!prefs().notifications.screenShares || !room || focused || userId === currentUser()?.id) return;
  if (isDoNotDisturb() || isChannelMuted(room.id)) return;
  const sharer = allVoiceParticipants().find((participant) => participant.user_id === userId);
  if (!sharer || sharer.channel_id !== room.id) return;
  void showNotice({
    title: t('{name} started sharing the screen', { name: sharer.username }),
    body: '',
    avatarUserId: userId,
    target: { kind: 'voice_room', id: room.id },
  });
}

/** A call to my room: a card in the app and, in the background, a notification with "Лететь" and "Остаться". */
export function reactToFlockCall(call: FlockCall, focused = document.hasFocus()) {
  if (!prefs().notifications.flockCalls || focused || isDoNotDisturb() || isChannelMuted(call.channel_id)) return;
  const room = channels().find((channel) => channel.id === call.channel_id)?.name ?? t('your voice room');
  void showNotice({
    title: t('{name} is calling you to {room}', { name: call.from_name, room }),
    body: '',
    avatarUserId: call.from_user_id,
    target: { kind: 'voice_room', id: call.channel_id },
    actions: [{ id: 'fly', label: t('Fly') }, { id: 'stay', label: t('Stay') }],
  });
}

/** A click on a notification: open its channel or room; a button answers the call to that room. */
export function handleNoticeActivated({ target, action }: NoticeActivation) {
  if (target.kind === 'channel' && target.id) {
    const channel = channels().find((candidate) => candidate.id === target.id);
    if (channel) void selectChannel(channel);
    return;
  }
  if (target.kind !== 'voice_room' || !target.id) return;
  const pending = pendingCallTo(target.id);
  if (action === 'fly') {
    if (pending) void acceptCall(pending.id);
    else void flyToRoom(target.id);
  } else if (action === 'stay' && pending) {
    dismissCall(pending.id);
  }
}

// ---- tray and window --------------------------------------------------------------------------

export type TrayState = 'idle' | 'voice' | 'speaking' | 'muted' | 'live';

/** The tray icon follows the current call and screen sharing state. */
export function trayStateNow(): TrayState {
  if (isSharing()) return 'live';
  if (!activeVoiceChannel()) return 'idle';
  if (isMuted() || isDeafened()) return 'muted';
  return isLocalSpeaking() ? 'speaking' : 'voice';
}

/** Hold the speaking icon through brief pauses, but apply mute, live and idle immediately. */
function createTrayStateHysteresis(readState: Accessor<TrayState>): Accessor<TrayState> {
  const requested = createMemo(readState);
  const [effective, setEffective] = createSignal(requested());
  createEffect(() => {
    const next = requested();
    if (next !== 'voice' || untrack(effective) !== 'speaking') {
      setEffective(next);
      return;
    }
    const release = setTimeout(() => setEffective('voice'), 400);
    onCleanup(() => clearTimeout(release));
  });
  return effective;
}

/** Mentions in channels that are not muted (the taskbar badge). */
export function mentionCount(): number {
  return Object.entries(unread())
    .filter(([channelId]) => !isChannelMuted(channelId))
    .reduce((sum, [, entry]) => sum + (entry.mentions ?? (entry.mention ? 1 : 0)), 0);
}

let started = false;
/** Keep the tray icon, the close behaviour and notification clicks in step with the app (once, for the life of the app). */
export function startDesktopSync() {
  if (started) return;
  started = true;
  createRoot(() => {
    const state = createTrayStateHysteresis(trayStateNow);
    // A language change also refreshes the native tooltip and menu.
    const language = createMemo(() => prefs().appearance.language);
    createEffect(() => {
      void api.setTrayState(state(), language()).catch(() => {});
    });
    createEffect(() => {
      void api.setAttention(mentionCount(), prefs().appearance.language).catch(() => {});
    });
  });
  // The backend has already brought the window up; this only moves to the right place.
  void listen<NoticeActivation>('notice-activated', (event) => handleNoticeActivated(event.payload)).catch(() => {});
}
