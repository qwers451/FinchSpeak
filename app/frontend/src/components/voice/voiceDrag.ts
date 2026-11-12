import { createSignal } from 'solid-js';
import { mayModerate } from '../../stores/adminStore';
import { allVoiceParticipants } from '../../stores/voiceStore';
import { localPeerId } from '../../stores/sessionStore';
import type { Channel, VoiceParticipant } from '../../types';
import { isMeParticipant } from './ParticipantTile';

/** Drag payload for moving a participant between voice rooms (sidebar rows and nests). */
export const VOICE_USER_DRAG = 'application/x-finch-voice-user';
export type VoiceDrag = { userId: string; peerId: number; from: string };

/** The drag in progress, shared across components: dragover cannot read the transfer data, only its types. */
export const [activeVoiceDrag, setActiveVoiceDrag] = createSignal<VoiceDrag | null>(null);

export function readVoiceDrag(transfer: DataTransfer | null): VoiceDrag | null {
  if (!transfer?.types.includes(VOICE_USER_DRAG) || transfer.types.includes('Files')) return null;
  try {
    const value: unknown = JSON.parse(transfer.getData(VOICE_USER_DRAG));
    if (value && typeof value === 'object' && 'userId' in value && typeof value.userId === 'string' &&
      'peerId' in value && Number.isSafeInteger(value.peerId) && 'from' in value && typeof value.from === 'string') return value as VoiceDrag;
  } catch { /* a different or malformed drag */ }
  return null;
}

/** Yourself always; anyone else only when you may moderate them. */
export function canDrag(participant: VoiceParticipant): boolean {
  return isMeParticipant(participant, localPeerId()) || mayModerate(participant.user_id);
}

/** The participant a drag would move into this channel, or null when the drop is not allowed. */
export function dropTarget(drag: VoiceDrag | null, channel: Channel): VoiceParticipant | null {
  if (!drag || channel.channel_type !== 'voice' || channel.id === drag.from) return null;
  const participant = allVoiceParticipants().find((person) => person.user_id === drag.userId && person.peer_id === drag.peerId && person.channel_id === drag.from);
  return participant && canDrag(participant) ? participant : null;
}
