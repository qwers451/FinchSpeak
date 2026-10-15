import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { Channel, Message, User, AudioDevice, VoiceParticipant } from '../types';

export const api = {
  // Auth
  login: (serverUrl: string, username: string, password: string): Promise<User> =>
    invoke('login', { serverUrl, username, password }),

  register: (serverUrl: string, username: string, displayName: string, password: string): Promise<User> =>
    invoke('register', { serverUrl, username, displayName, password }),

  logout: (): Promise<void> =>
    invoke('logout'),

  getCurrentUser: (): Promise<User | null> =>
    invoke('get_current_user'),

  trustFingerprint: (hostname: string, fingerprint: string): Promise<void> =>
    invoke('trust_fingerprint', { hostname, fingerprint }),

  // Channels & Chat
  listChannels: (): Promise<Channel[]> =>
    invoke('list_channels'),

  createChannel: (name: string, channelType: string, topic?: string, bitrateKbps?: number): Promise<Channel> =>
    invoke('create_channel', { name, channelType, topic, bitrateKbps }),

  fetchChannelMessages: (channelId: string, before?: number, limit?: number): Promise<{ channel_id: string; messages: Message[]; has_more: boolean }> =>
    invoke('fetch_channel_messages', { channelId, before, limit }),

  sendTextMessage: (channelId: string, content: string, attachments?: string[]): Promise<void> =>
    invoke('send_text_message', { channelId, content, attachments }),

  uploadFile: (filename: string, contentType: string, data: number[]): Promise<any> =>
    invoke('upload_file', { filename, contentType, data }),

  // Voice
  joinVoiceChannel: (channelId: string, shortId: number): Promise<void> =>
    invoke('join_voice_channel', { channelId, shortId }),

  leaveVoiceChannel: (): Promise<void> =>
    invoke('leave_voice_channel'),

  setVoiceState: (muted: boolean, deafened: boolean): Promise<void> =>
    invoke('set_voice_state', { muted, deafened }),

  getAudioDevices: (): Promise<{ inputs: AudioDevice[]; outputs: AudioDevice[] }> =>
    invoke('get_audio_devices'),

  setSpeakerVolume: (peerId: number, volume: number): Promise<void> =>
    invoke('set_speaker_volume', { peerId, volume }),

  // Event Listeners
  onNewMessage: (cb: (msg: Message) => void): Promise<UnlistenFn> =>
    listen('new-message', (e) => cb(e.payload as Message)),

  onUserSpeaking: (cb: (data: { peer_id: number; is_speaking: boolean }) => void): Promise<UnlistenFn> =>
    listen('user-speaking', (e) => cb(e.payload as any)),

  onLocalSpeaking: (cb: (isSpeaking: boolean) => void): Promise<UnlistenFn> =>
    listen('local-speaking', (e) => cb(e.payload as boolean)),

  onUserJoinedVoice: (cb: (data: VoiceParticipant) => void): Promise<UnlistenFn> =>
    listen('user-joined-voice', (e) => cb(e.payload as VoiceParticipant)),

  onUserLeftVoice: (cb: (data: { user_id: string; peer_id: number; channel_id: string }) => void): Promise<UnlistenFn> =>
    listen('user-left-voice', (e) => cb(e.payload as any)),

  onPingRtt: (cb: (data: { rtt_ms: number }) => void): Promise<UnlistenFn> =>
    listen('ping-rtt', (e) => cb(e.payload as any)),
};
