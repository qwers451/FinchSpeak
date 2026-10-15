import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { Channel, Message, User, AudioDevice, VoiceParticipant } from '../types';

const isTauri = () => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

let browserToken: string | null = null;
let browserServerUrl = 'http://127.0.0.1:8443';
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

  register: async (serverUrl: string, username: string, displayName: string, password: string): Promise<User> => {
    if (isTauri()) {
      return invoke('register', { serverUrl, username, displayName, password });
    }
    const cleanUrl = serverUrl.replace(/\/+$/, '');
    const resp = await fetch(`${cleanUrl}/api/auth/register`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ username, display_name: displayName, password }),
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
    browserToken = null;
    browserUser = null;
  },

  getCurrentUser: async (): Promise<User | null> => {
    if (isTauri()) {
      return invoke('get_current_user');
    }
    return browserUser;
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

  fetchChannelMessages: async (channelId: string, before?: number, limit?: number): Promise<{ channel_id: string; messages: Message[]; has_more: boolean }> => {
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

  sendTextMessage: async (channelId: string, content: string, attachments?: string[]): Promise<void> => {
    if (isTauri()) {
      return invoke('send_text_message', { channelId, content, attachments });
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

  getAudioDevices: async (): Promise<{ inputs: AudioDevice[]; outputs: AudioDevice[] }> => {
    if (isTauri()) {
      return invoke('get_audio_devices');
    }
    return { inputs: [], outputs: [] };
  },

  setSpeakerVolume: async (peerId: number, volume: number): Promise<void> => {
    if (isTauri()) {
      return invoke('set_speaker_volume', { peerId, volume });
    }
  },

  // Event Listeners
  onNewMessage: async (cb: (msg: Message) => void): Promise<UnlistenFn> => {
    if (isTauri()) {
      return listen('new-message', (e) => cb(e.payload as Message));
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

  onPingRtt: async (cb: (data: { rtt_ms: number }) => void): Promise<UnlistenFn> => {
    if (isTauri()) {
      return listen('ping-rtt', (e) => cb(e.payload as any));
    }
    return () => {};
  },
};
