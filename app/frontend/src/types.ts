export interface User {
  id: string;
  username: string;
  display_name: string;
  role: 'admin' | 'member';
  avatar_url?: string | null;
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
  message_id: string;
  channel_id: string;
  author_id: string;
  author_name: string;
  content: string;
  attachments: AttachmentInfo[];
  timestamp: number;
}

export interface VoiceParticipant {
  user_id: string;
  peer_id: number;
  username: string;
  channel_id: string;
  is_muted?: boolean;
  is_deafened?: boolean;
  is_speaking?: boolean;
}

export interface AudioDevice {
  name: string;
  is_default: boolean;
}
