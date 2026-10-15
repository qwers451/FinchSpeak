export interface User {
  id: string;
  username: string;
  display_name: string;
  role: 'admin' | 'member';
  avatar_url?: string | null;
  status?: UserStatus;
}

export interface Channel {
  id: string;
  short_id: number;
  name: string;
  channel_type: 'text' | 'voice';
  position: number;
  topic?: string | null;
  bitrate_kbps: number;
}

export interface AttachmentInfo {
  id: string;
  filename: string;
  size_bytes: number;
  content_type: string;
  url: string;
}

export interface Message {
  id: string;
  channel_id: string;
  author_id: string;
  author_name: string;
  content: string;
  attachments: AttachmentInfo[];
  timestamp: number;
  /** The id the author chose when sending; ties a pending message to the stored one. */
  client_msg_id?: string | null;
}

export type IncomingMessage = Omit<Message, 'id'> & { message_id: string };
export type HistoryMessage = Omit<Message, 'channel_id'>;
export interface MessageHistory {
  channel_id: string;
  messages: HistoryMessage[];
  has_more: boolean;
}

export interface VoiceParticipant {
  user_id: string;
  peer_id: number;
  username: string;
  channel_id: string;
  is_muted?: boolean;
  is_deafened?: boolean;
  is_speaking?: boolean;
  /** Currently sharing their screen. */
  is_sharing?: boolean;
}

export interface MonitorSource {
  id: number;
  name: string;
  width: number;
  height: number;
  is_primary: boolean;
}

export interface WindowSource {
  id: number;
  title: string;
  app_name: string;
  width: number;
  height: number;
}

export interface ScreenSources {
  monitors: MonitorSource[];
  windows: WindowSource[];
}

/** What the user is watching. */
export interface WatchedShare {
  user_id: string;
  peer_id: number;
}

export interface AudioDevice {
  name: string;
  is_default: boolean;
}

export type UserStatus = 'online' | 'idle' | 'donotdisturb' | 'offline';
export interface SessionEvents {
  authenticated: {
    session_peer_id: number;
    user_id: string;
    channels: { id: string; short_id: number }[];
    initial_state: {
      channels: Channel[];
      online_users: User[];
      voice_states: (Omit<VoiceParticipant, 'is_muted' | 'is_deafened' | 'is_sharing'> & {
        muted: boolean; deafened: boolean; sharing_screen: boolean;
      })[];
    };
  };
  'new-message': IncomingMessage;
  'message-rejected': { client_msg_id: string; code: number; message: string };
  'user-speaking': { peer_id: number; is_speaking: boolean };
  'local-speaking': boolean;
  'ping-rtt': { rtt_ms: number };
  'tofu-required': { hostname: string; fingerprint: string };
  'channel-created': { channel: Channel };
  'user-joined-voice': VoiceParticipant;
  'user-left-voice': { user_id: string; peer_id: number; channel_id: string };
  'user-voice-state-changed': { peer_id: number; muted: boolean; deafened: boolean };
  'screen-share-started': { user_id: string; peer_id: number; width: number; height: number; fps: number };
  'screen-share-stopped': { peer_id: number };
  /** Our own share ended by itself (capture failed, window closed). */
  'screen-share-ended': string;
  /** The sound of the shared screen could not be captured; the picture still goes out. */
  'screen-audio-failed': string;
  'presence-update': { user_id: string; status: UserStatus };
  'connection-lost': string;
  reconnecting: null;
  reconnected: null;
  'reconnect-failed': string;
  'server-error': { code: number; message: string };
  'audio-device-lost': null;
  'audio-device-failed': null;
  'audio-device-fallback': { kind: 'input' | 'output'; wanted: string };
}

export interface AudioDevices {
  inputs: AudioDevice[];
  outputs: AudioDevice[];
  /** What the backend uses; null = system default. */
  selected_input: string | null;
  selected_output: string | null;
}

export interface SessionSnapshot {
  connected: boolean;
  user_id: string;
  session_peer_id: number;
  current_voice_channel: number | null;
  muted: boolean;
  deafened: boolean;
  pending_tofu: { hostname: string; fingerprint: string } | null;
  initial_state: SessionEvents['authenticated']['initial_state'] | null;
}
