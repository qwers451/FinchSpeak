import { createSignal } from 'solid-js';
import type { SessionEvents, SessionSnapshot, User, UserStatus } from '../types';
import { setPendingMismatch, setPendingTofu } from './tofuStore';
import { activeChannel, channels, handleChannelCreated, handleChannelDeleted, handleChannelUpdated, handleIncomingMessage, handleMessageRejected, selectChannel, setChannels } from './chatStore';
import { activeVoiceChannel, handleUserSpeaking, leaveVoice, setActiveVoiceChannel, setIsMuted, setIsDeafened, setIsLocalSpeaking, setPingRtt, setSpeakingPeers, setVoiceParticipants } from './voiceStore';
import { setAppError } from './notificationStore';
import { clearTyping, noteIncoming, noteTyping, resetActivity, setUnreadFromServer } from './activityStore';
import { handleMessageDeleted, handleMessageEdited, handleReactionChanged, renameAuthor } from './chatStore';
import { currentUser, setCurrentUser } from './authStore';
import { resetScreenState, resetScreenStateOnDisconnect, setWatching, watching } from './screenState';
import { screenHandlers } from './screenStore';
export { appError, setAppError } from './notificationStore';

export const [connectionStatus, setConnectionStatus] = createSignal<'connected' | 'disconnected' | 'reconnecting' | 'failed'>('disconnected');
export const [connectionError, setConnectionError] = createSignal<string | null>(null);
/** Failed reconnect tries so far and the wait before the next one. */
export const [reconnectInfo, setReconnectInfo] = createSignal<{ attempt: number; retryInSecs: number } | null>(null);
export const [localPeerId, setLocalPeerId] = createSignal<number | null>(null);
export const [sessionUserId, setSessionUserId] = createSignal<string | null>(null);
export const [knownUsers, setKnownUsers] = createSignal<User[]>([]);
export const [presence, setPresence] = createSignal<Record<string, UserStatus>>({});
export const onlineUsers = () => knownUsers().filter((user) => (presence()[user.id] ?? user.status) !== 'offline');

export function resetSessionState() {
  setConnectionStatus('disconnected');
  setConnectionError(null);
  setReconnectInfo(null);
  setLocalPeerId(null);
  setSessionUserId(null);
  setKnownUsers([]);
  setPresence({});
  setVoiceParticipants([]);
  setSpeakingPeers(new Set<number>());
  setIsLocalSpeaking(false);
  setPingRtt(null);
  resetScreenState();
  resetActivity();
  setAppError(null);
}

export const sessionHandlers: { [K in keyof SessionEvents]: (payload: SessionEvents[K]) => void } = {
  authenticated: (data) => {
    setSessionUserId(data.user_id);
    setLocalPeerId(data.session_peer_id);
    setChannels(data.initial_state.channels);
    setKnownUsers(data.initial_state.online_users);
    setPresence(Object.fromEntries(data.initial_state.online_users.map((user) => [user.id, user.status ?? 'online'])));
    setVoiceParticipants(data.initial_state.voice_states.map(({ muted, deafened, sharing_screen, ...participant }) => ({
      ...participant, is_muted: muted, is_deafened: deafened, is_sharing: sharing_screen,
    })));
    setConnectionStatus('connected');
    setConnectionError(null);
    setReconnectInfo(null);
    setUnreadFromServer(data.initial_state.unread);
    const watched = watching();
    if (watched) {
      const sharer = data.initial_state.voice_states.find((state) => state.user_id === watched.user_id && state.sharing_screen);
      if (sharer) setWatching({ user_id: watched.user_id, peer_id: sharer.peer_id });
      else setWatching(null);
    }
  },
  'new-message': (incoming) => {
    handleIncomingMessage(incoming);
    clearTyping(incoming.channel_id, incoming.author_id);
    noteIncoming(incoming);
  },
  'message-edited': handleMessageEdited,
  'message-deleted': handleMessageDeleted,
  'reaction-changed': handleReactionChanged,
  'user-typing': (event) => noteTyping(event),
  'user-profile-changed': ({ user_id, display_name }) => {
    setKnownUsers((previous) => previous.map((user) => (user.id === user_id ? { ...user, display_name } : user)));
    setVoiceParticipants((previous) => previous.map((p) => (p.user_id === user_id ? { ...p, username: display_name } : p)));
    renameAuthor(user_id, display_name);
    const me = currentUser();
    if (me && me.id === user_id) setCurrentUser({ ...me, display_name });
  },
  'message-rejected': handleMessageRejected,
  'user-speaking': (data) => handleUserSpeaking(data.peer_id, data.is_speaking),
  'local-speaking': setIsLocalSpeaking,
  'ping-rtt': (data) => setPingRtt(data.rtt_ms),
  'tofu-required': setPendingTofu,
  'tofu-mismatch': setPendingMismatch,
  'reconnect-attempt': ({ attempt, retry_in_secs }) => setReconnectInfo({ attempt, retryInSecs: retry_in_secs }),
  'channel-created': ({ channel }) => handleChannelCreated(channel),
  'channel-updated': ({ channel }) => handleChannelUpdated(channel),
  'channel-deleted': ({ channel_id }) => {
    handleChannelDeleted(channel_id);
    if (activeVoiceChannel()?.id === channel_id) void leaveVoice();
  },
  'user-joined-voice': (data) => setVoiceParticipants((previous) => [
    ...previous.filter((participant) => participant.peer_id !== data.peer_id), data,
  ]),
  'user-left-voice': (data) => {
    setVoiceParticipants((previous) => previous.filter((participant) => participant.peer_id !== data.peer_id || participant.channel_id !== data.channel_id));
    handleUserSpeaking(data.peer_id, false);
    if (watching()?.peer_id === data.peer_id) setWatching(null);
  },
  'user-voice-state-changed': (data) => setVoiceParticipants((previous) => previous.map((participant) =>
    participant.peer_id === data.peer_id ? { ...participant, is_muted: data.muted, is_deafened: data.deafened } : participant)),
  ...screenHandlers,
  'presence-update': (data) => setPresence((previous) => ({ ...previous, [data.user_id]: data.status })),
  'connection-lost': (reason) => {
    setConnectionStatus('disconnected');
    setConnectionError(reason);
    setVoiceParticipants([]);
    setSpeakingPeers(new Set<number>());
    setIsLocalSpeaking(false);
    setPingRtt(null);
    resetScreenStateOnDisconnect();
  },
  reconnecting: () => setConnectionStatus('reconnecting'),
  reconnected: () => {
    setConnectionStatus('connected'); setConnectionError(null); setReconnectInfo(null);
    const channel = activeChannel();
    if (channel?.channel_type === 'text') void selectChannel(channel);
  },
  'reconnect-failed': (reason) => { setConnectionStatus('failed'); setConnectionError(reason); },
  'server-error': (error) => setAppError(`${error.code}: ${error.message}`),
  'audio-device-lost': () => setAppError('An audio device was disconnected; reopening the audio devices...'),
  'audio-device-failed': () => setAppError('The audio devices keep failing. Check them, then rejoin the voice channel.'),
  'audio-device-fallback': ({ kind, wanted }) =>
    setAppError(`"${wanted}" is not available; using the system default ${kind} device instead.`),
};

export function restoreSessionSnapshot(snapshot: SessionSnapshot) {
  if (snapshot.connected && snapshot.initial_state) {
    sessionHandlers.authenticated({ user_id: snapshot.user_id, session_peer_id: snapshot.session_peer_id,
      channels: snapshot.initial_state.channels, initial_state: snapshot.initial_state });
  } else {
    setConnectionStatus('disconnected');
  }
  setIsMuted(snapshot.muted);
  setIsDeafened(snapshot.deafened);
  setActiveVoiceChannel(channels().find((channel) => channel.channel_type === 'voice' &&
    channel.short_id === snapshot.current_voice_channel) ?? null);
  setPendingTofu(snapshot.pending_tofu);
}
