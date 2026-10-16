export type Role = 'admin' | 'moderator' | 'member';

export interface User {
  id: string;
  username: string;
  display_name: string;
  role: Role;
  avatar_url?: string | null;
  status?: UserStatus;
  status_text?: string | null;
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
  width?: number | null;
  height?: number | null;
  /** The server made a 320 px preview. */
  has_thumb?: boolean;
}

/** The message a reply answers (as the server quotes it). */
export interface ReplyRef {
  message_id: string;
  author_name: string;
  snippet: string;
  deleted?: boolean;
}

export interface ChannelUnread {
  channel_id: string;
  count: number;
  mentions: number;
  first_unread_id?: string | null;
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
  edited_at?: number | null;
  reactions?: Reaction[];
  reply_to?: ReplyRef | null;
  /** Ids of the users the text mentions. */
  mentions?: string[];
  /** Not stored by the server yet: `sending`, or `failed` with the reason in `failure`. */
  state?: 'sending' | 'failed';
  failure?: string;
}

export interface Reaction {
  emoji: string;
  user_ids: string[];
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
  /** A moderator silenced this person. */
  server_muted?: boolean;
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

export interface AdminUser {
  id: string;
  username: string;
  display_name: string;
  role: Role;
  banned: boolean;
  /** When a timed ban ends (unix seconds); null for a permanent one. */
  banned_until?: number | null;
  online: boolean;
  last_seen?: number | null;
}

export interface AuditEntry {
  id: number;
  at: number;
  actor_name: string;
  action: string;
  target?: string | null;
  details?: string | null;
}

export interface ServerOverview {
  version: string;
  uptime_secs: number;
  certificate_fingerprint: string | null;
  quic_port: number;
  http_port: number;
  registration: 'open' | 'invite';
  backups_count: number;
  last_backup_at: number | null;
  last_backup_bytes: number | null;
  backup_dir: string;
  uploads_bytes: number;
  log_dir: string;
}

export interface SearchHit {
  message_id: string;
  channel_id: string;
  channel_name: string;
  author_id: string;
  author_name: string;
  content: string;
  timestamp: number;
}

export interface Invite {
  code: string;
  created_at: number;
  expires_at: number;
  max_uses: number;
  uses: number;
}

export interface VoiceSettings {
  /** Microphone volume multiplier, 1 = unchanged. */
  input_gain: number;
  /** Speech detector loudness threshold in dBFS. */
  vad_threshold_db: number;
  push_to_talk: boolean;
  /** The push-to-talk key is held down right now. */
  key_down: boolean;
  /** Hear your own microphone in the speakers. */
  monitor: boolean;
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
      unread?: ChannelUnread[];
    };
  };
  'new-message': IncomingMessage;
  'message-rejected': { client_msg_id: string; code: number; message: string };
  'user-speaking': { peer_id: number; is_speaking: boolean };
  'local-speaking': boolean;
  'ping-rtt': { rtt_ms: number };
  'tofu-required': { hostname: string; fingerprint: string };
  'tofu-mismatch': { hostname: string; expected: string; actual: string };
  'reconnect-attempt': { attempt: number; retry_in_secs: number };
  'channel-created': { channel: Channel };
  'user-joined-voice': VoiceParticipant;
  'user-left-voice': { user_id: string; peer_id: number; channel_id: string };
  'message-edited': { message_id: string; channel_id: string; content: string; edited_at: number };
  'message-deleted': { message_id: string; channel_id: string };
  'reaction-changed': { message_id: string; channel_id: string; user_id: string; emoji: string; added: boolean };
  'user-typing': { channel_id: string; user_id: string; username: string };
  'user-profile-changed': { user_id: string; display_name: string };
  'channel-updated': { channel: Channel };
  'channel-deleted': { channel_id: string };
  'user-voice-state-changed': { peer_id: number; muted: boolean; deafened: boolean };
  'screen-share-started': { user_id: string; peer_id: number; width: number; height: number; fps: number };
  'screen-share-stopped': { peer_id: number; reason?: string };
  /** Our own share ended by itself (capture failed, window closed). */
  'screen-share-ended': string;
  /** The sound of the shared screen could not be captured; the picture still goes out. */
  'screen-audio-failed': string;
  'presence-update': { user_id: string; status: UserStatus; status_text?: string | null };
  'avatar-changed': { user_id: string; avatar_url: string | null };
  'server-mute-changed': { peer_id: number; server_muted: boolean };
  'voice-kicked': { channel_id: string };
  /** Level of the processed microphone, about 25 times a second during a call. */
  'mic-level': { db: number; speaking: boolean };
  'mic-test': { phase: 'recording' | 'playing' | 'done'; seconds?: number };
  'voice-quality': { peer_id: number; quality: 'good' | 'fair' | 'poor'; loss: number }[];
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
