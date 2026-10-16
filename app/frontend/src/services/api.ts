import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { AdminUser, Channel, IncomingMessage, Invite, MessageHistory, ScreenSources, User, AudioDevices, VoiceParticipant, SessionEvents, SessionSnapshot } from '../types';

const isTauri = () => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

let browserToken: string | null = null;
let browserServerUrl = 'https://127.0.0.1:8443';
let browserUser: User | null = null;

export const api = {
  // Auth
  login: async (serverUrl: string, username: string, password: string): Promise<User> => {
    if (isTauri()) {
      return invoke('login', { serverUrl, username, password });
    }
    const cleanUrl = serverUrl.replace(/\/+$/, '');
    const resp = await fetch(`${cleanUrl}/api/auth/login`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ username, password }),
    });
    if (!resp.ok) {
      const err = await resp.text();
      throw new Error(`Login failed: ${err}`);
    }
    const data = await resp.json();
    browserToken = data.token;
    browserServerUrl = cleanUrl;
    browserUser = data.user;
    return data.user;
  },

  register: async (serverUrl: string, username: string, displayName: string, password: string, inviteCode?: string): Promise<User> => {
    if (isTauri()) {
      return invoke('register', { serverUrl, username, displayName, password, inviteCode: inviteCode || null });
    }
    const cleanUrl = serverUrl.replace(/\/+$/, '');
    const resp = await fetch(`${cleanUrl}/api/auth/register`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ username, display_name: displayName, password, invite_code: inviteCode || null }),
    });
    if (!resp.ok) {
      const err = await resp.text();
      throw new Error(`Registration failed: ${err}`);
    }
    const data = await resp.json();
    browserToken = data.token;
    browserServerUrl = cleanUrl;
    browserUser = data.user;
    return data.user;
  },

  logout: async (): Promise<void> => {
    if (isTauri()) {
      return invoke('logout');
    }
    try {
      if (browserToken) {
        const response = await fetch(`${browserServerUrl}/api/auth/logout`, {
          method: 'POST', headers: { Authorization: `Bearer ${browserToken}` },
        });
        if (!response.ok && response.status !== 401) throw new Error(`Logout failed: ${response.status}`);
      }
    } finally {
      browserToken = null;
      browserUser = null;
    }
  },

  getCurrentUser: async (): Promise<User | null> => {
    if (isTauri()) {
      return invoke('get_current_user');
    }
    return browserUser;
  },

  getSessionSnapshot: async (): Promise<SessionSnapshot | null> => {
    if (isTauri()) return invoke('get_session_snapshot');
    return null;
  },

  trustFingerprint: async (hostname: string, fingerprint: string): Promise<void> => {
    if (isTauri()) {
      return invoke('trust_fingerprint', { hostname, fingerprint });
    }
  },

  // Channels & Chat
  listChannels: async (): Promise<Channel[]> => {
    if (isTauri()) {
      return invoke('list_channels');
    }
    const resp = await fetch(`${browserServerUrl}/api/channels`, {
      headers: { Authorization: `Bearer ${browserToken}` },
    });
    if (!resp.ok) throw new Error(`Failed to list channels: ${resp.status}`);
    return resp.json();
  },

  createChannel: async (name: string, channelType: string, topic?: string, bitrateKbps?: number): Promise<Channel> => {
    if (isTauri()) {
      return invoke('create_channel', { name, channelType, topic, bitrateKbps });
    }
    const resp = await fetch(`${browserServerUrl}/api/channels`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        Authorization: `Bearer ${browserToken}`,
      },
      body: JSON.stringify({ name, channel_type: channelType, topic, bitrate_kbps: bitrateKbps }),
    });
    if (!resp.ok) throw new Error(`Failed to create channel: ${resp.status}`);
    return resp.json();
  },

  fetchChannelMessages: async (channelId: string, before?: number, limit?: number): Promise<MessageHistory> => {
    if (isTauri()) {
      return invoke('fetch_channel_messages', { channelId, before, limit });
    }
    let url = `${browserServerUrl}/api/channels/${channelId}/messages?limit=${limit || 50}`;
    if (before) url += `&before=${before}`;
    const resp = await fetch(url, {
      headers: { Authorization: `Bearer ${browserToken}` },
    });
    if (!resp.ok) throw new Error(`Failed to fetch messages: ${resp.status}`);
    return resp.json();
  },

  sendTextMessage: async (channelId: string, content: string, attachments?: string[], clientMsgId?: string): Promise<void> => {
    if (isTauri()) {
      return invoke('send_text_message', { channelId, content, attachments, clientMsgId });
    }
    // In browser environment, broadcast or notify
    console.info(`[Browser mode] Simulated message sent to ${channelId}: ${content}`);
  },

  uploadFile: async (filename: string, contentType: string, data: number[]): Promise<any> => {
    if (isTauri()) {
      return invoke('upload_file', { filename, contentType, data });
    }
    const blob = new Blob([new Uint8Array(data)], { type: contentType });
    const formData = new FormData();
    formData.append('file', blob, filename);
    const resp = await fetch(`${browserServerUrl}/api/files/upload`, {
      method: 'POST',
      headers: { Authorization: `Bearer ${browserToken}` },
      body: formData,
    });
    if (!resp.ok) throw new Error(`Upload failed: ${resp.status}`);
    return resp.json();
  },

  /** The bytes of an attachment. */
  downloadFile: async (id: string): Promise<ArrayBuffer> => {
    if (isTauri()) {
      return invoke('download_file', { id });
    }
    const resp = await fetch(`${browserServerUrl}/api/files/${id}`, { headers: { Authorization: `Bearer ${browserToken}` } });
    if (!resp.ok) throw new Error(`Download failed: ${resp.status}`);
    return resp.arrayBuffer();
  },

  editMessage: async (messageId: string, content: string): Promise<void> => {
    if (isTauri()) {
      return invoke('edit_message', { messageId, content });
    }
  },

  deleteMessage: async (messageId: string): Promise<void> => {
    if (isTauri()) {
      return invoke('delete_message', { messageId });
    }
  },

  setReaction: async (messageId: string, emoji: string, on: boolean): Promise<void> => {
    if (isTauri()) {
      return invoke('set_reaction', { messageId, emoji, on });
    }
  },

  sendTyping: async (channelId: string): Promise<void> => {
    if (isTauri()) {
      return invoke('send_typing', { channelId });
    }
  },

  updateProfile: async (displayName: string): Promise<User> => {
    if (isTauri()) {
      return invoke('update_profile', { displayName });
    }
    throw new Error('Profile changes need the desktop app');
  },

  changePassword: async (currentPassword: string, newPassword: string): Promise<void> => {
    if (isTauri()) {
      return invoke('change_password', { currentPassword, newPassword });
    }
    throw new Error('Password changes need the desktop app');
  },

  // Voice
  joinVoiceChannel: async (channelId: string, shortId: number): Promise<void> => {
    if (isTauri()) {
      return invoke('join_voice_channel', { channelId, shortId });
    }
  },

  leaveVoiceChannel: async (): Promise<void> => {
    if (isTauri()) {
      return invoke('leave_voice_channel');
    }
  },

  setVoiceState: async (muted: boolean, deafened: boolean): Promise<void> => {
    if (isTauri()) {
      return invoke('set_voice_state', { muted, deafened });
    }
  },

  getAudioDevices: async (): Promise<AudioDevices> => {
    if (isTauri()) {
      return invoke('get_audio_devices');
    }
    return { inputs: [], outputs: [], selected_input: null, selected_output: null };
  },

  /** Choose microphone and speakers (null = system default); reopens the streams if in a call. */
  setAudioDevices: async (input: string | null, output: string | null): Promise<void> => {
    if (isTauri()) {
      return invoke('set_audio_devices', { input, output });
    }
  },

  // Administration
  createInvite: async (maxUses: number, expiresHours: number): Promise<Invite> => {
    if (isTauri()) {
      return invoke('create_invite', { maxUses, expiresHours });
    }
    throw new Error('Administration needs the desktop app');
  },

  listInvites: async (): Promise<Invite[]> => {
    if (isTauri()) {
      return invoke('list_invites');
    }
    return [];
  },

  revokeInvite: async (code: string): Promise<void> => {
    if (isTauri()) {
      return invoke('revoke_invite', { code });
    }
  },

  adminUsers: async (): Promise<AdminUser[]> => {
    if (isTauri()) {
      return invoke('admin_users');
    }
    return [];
  },

  setUserRole: async (userId: string, role: 'admin' | 'member'): Promise<void> => {
    if (isTauri()) {
      return invoke('set_user_role', { userId, role });
    }
  },

  kickUser: async (userId: string): Promise<void> => {
    if (isTauri()) {
      return invoke('kick_user', { userId });
    }
  },

  banUser: async (userId: string): Promise<void> => {
    if (isTauri()) {
      return invoke('ban_user', { userId });
    }
  },

  unbanUser: async (userId: string): Promise<void> => {
    if (isTauri()) {
      return invoke('unban_user', { userId });
    }
  },

  updateChannel: async (channelId: string, name?: string, topic?: string): Promise<void> => {
    if (isTauri()) {
      return invoke('update_channel', { channelId, name: name ?? null, topic: topic ?? null });
    }
  },

  deleteChannel: async (channelId: string): Promise<void> => {
    if (isTauri()) {
      return invoke('delete_channel', { channelId });
    }
  },

  reorderChannels: async (channelIds: string[]): Promise<void> => {
    if (isTauri()) {
      return invoke('reorder_channels', { channelIds });
    }
  },

  // Screen sharing
  listScreenSources: async (): Promise<ScreenSources> => {
    if (isTauri()) {
      return invoke('list_screen_sources');
    }
    return { monitors: [], windows: [] };
  },

  startScreenShare: async (kind: 'monitor' | 'window', id: number, withAudio = false): Promise<void> => {
    if (isTauri()) {
      return invoke('start_screen_share', { kind, id, withAudio });
    }
  },

  stopScreenShare: async (): Promise<void> => {
    if (isTauri()) {
      return invoke('stop_screen_share');
    }
  },

  subscribeScreenShare: async (targetUserId: string, targetPeerId: number): Promise<void> => {
    if (isTauri()) {
      return invoke('subscribe_screen_share', { targetUserId, targetPeerId });
    }
  },

  unsubscribeScreenShare: async (targetUserId: string): Promise<void> => {
    if (isTauri()) {
      return invoke('unsubscribe_screen_share', { targetUserId });
    }
  },

  setSpeakerVolume: async (peerId: number, volume: number): Promise<void> => {
    if (isTauri()) {
      return invoke('set_speaker_volume', { peerId, volume });
    }
  },

  // Event Listeners
  onSessionEvent: async <K extends keyof SessionEvents>(event: K, callback: (payload: SessionEvents[K]) => void): Promise<UnlistenFn> => {
    if (isTauri()) return listen<SessionEvents[K]>(event, ({ payload }) => callback(payload));
    return () => {};
  },
  onNewMessage: async (cb: (msg: IncomingMessage) => void): Promise<UnlistenFn> => {
    if (isTauri()) {
      return listen<IncomingMessage>('new-message', (e) => cb(e.payload));
    }
    return () => {};
  },

  onUserSpeaking: async (cb: (data: { peer_id: number; is_speaking: boolean }) => void): Promise<UnlistenFn> => {
    if (isTauri()) {
      return listen('user-speaking', (e) => cb(e.payload as any));
    }
    return () => {};
  },

  onLocalSpeaking: async (cb: (isSpeaking: boolean) => void): Promise<UnlistenFn> => {
    if (isTauri()) {
      return listen('local-speaking', (e) => cb(e.payload as boolean));
    }
    return () => {};
  },

  onUserJoinedVoice: async (cb: (data: VoiceParticipant) => void): Promise<UnlistenFn> => {
    if (isTauri()) {
      return listen('user-joined-voice', (e) => cb(e.payload as VoiceParticipant));
    }
    return () => {};
  },

  onUserLeftVoice: async (cb: (data: { user_id: string; peer_id: number; channel_id: string }) => void): Promise<UnlistenFn> => {
    if (isTauri()) {
      return listen('user-left-voice', (e) => cb(e.payload as any));
    }
    return () => {};
  },

  onChannelCreated: async (cb: (channel: Channel) => void): Promise<UnlistenFn> => {
    if (isTauri()) {
      return listen('channel-created', (e) => cb((e.payload as { channel: Channel }).channel));
    }
    return () => {};
  },

  onTofuRequired: async (cb: (data: { hostname: string; fingerprint: string }) => void): Promise<UnlistenFn> => {
    if (isTauri()) {
      return listen('tofu-required', (e) => cb(e.payload as any));
    }
    return () => {};
  },

  onPingRtt: async (cb: (data: { rtt_ms: number }) => void): Promise<UnlistenFn> => {
    if (isTauri()) {
      return listen('ping-rtt', (e) => cb(e.payload as any));
    }
    return () => {};
  },
};
