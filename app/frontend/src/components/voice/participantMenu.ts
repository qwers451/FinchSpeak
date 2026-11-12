import { currentUser } from '../../stores/authStore';
import { knownUsers, localPeerId } from '../../stores/sessionStore';
import { allVoiceParticipants, isMuted, isDeafened, toggleMute, toggleDeafen } from '../../stores/voiceStore';
import { channels } from '../../stores/chatStore';
import { mayModerate, moderateVoice, moveVoice } from '../../stores/adminStore';
import { volumeFor, setVolume } from '../../stores/volumeStore';
import { isLocallyMuted, toggleLocalMute } from '../../stores/localMuteStore';
import { openContextMenu, type MenuItem } from '../../ui/ContextMenu';
import type { PopoverAnchor } from '../../ui/popover';
import type { VoiceParticipant } from '../../types';
import { t } from '../../i18n';

export function openParticipantMenu(anchor: PopoverAnchor, userId: string, participant?: VoiceParticipant) {
  const user = () => knownUsers().find((candidate) => candidate.id === userId);
  const voice = () => allVoiceParticipants().find((p) => p.user_id === userId && (participant ? p.peer_id === participant.peer_id : true));
  const self = () => participant ? participant.peer_id === localPeerId() || (localPeerId() === null && userId === currentUser()?.id) : userId === currentUser()?.id;
  const name = user()?.display_name || participant?.username || user()?.username || '';
  const items: MenuItem[] = self() ? [
    { kind: 'item', label: t(isMuted() ? 'Unmute' : 'Mute'), icon: 'mic', onSelect: () => void toggleMute() },
    { kind: 'item', label: t(isDeafened() ? 'Undeafen' : 'Deafen'), icon: 'phones', onSelect: () => void toggleDeafen() },
  ] : [
    { kind: 'header', label: name },
    { kind: 'slider', label: t('Volume'), min: 0, max: 2, step: 0.05, value: volumeFor(userId),
      disabled: isLocallyMuted(userId), format: (value) => `${Math.round(value * 100)}%`,
      onChange: (value) => void setVolume(userId, voice()?.peer_id, value) },
    { kind: 'item', label: t('Mute for me'), icon: 'vol-off', checked: isLocallyMuted(userId),
      onSelect: () => void toggleLocalMute(userId, voice()?.peer_id) },
  ];
  const canModerate = () => !self() && !!voice() && mayModerate(userId);
  if (canModerate()) {
    items.push(
      { kind: 'separator' },
      { kind: 'item', label: t(voice()?.server_muted ? 'Allow to speak' : 'Silence for everyone'), icon: 'shield',
        onSelect: () => { if (canModerate()) void moderateVoice(userId, voice()?.server_muted ? 'unmute' : 'mute'); } },
      { kind: 'submenu', label: t('Move to…'), icon: 'vol',
        items: channels().filter((room) => room.channel_type === 'voice' && room.id !== voice()?.channel_id)
          .map((room) => ({ kind: 'item', label: room.name, onSelect: () => {
            const target = voice();
            if (canModerate() && target && target.channel_id !== room.id && channels().some((channel) => channel.id === room.id && channel.channel_type === 'voice')) {
              void moveVoice(userId, target.peer_id, room.id);
            }
          } })) },
      { kind: 'item', label: t('Remove from the room'), icon: 'door', danger: true,
        onSelect: () => { if (canModerate()) void moderateVoice(userId, 'kick'); } },
    );
  }
  openContextMenu(anchor, items);
}

export function participantContextMenu(event: MouseEvent, userId: string, participant?: VoiceParticipant) {
  event.preventDefault();
  event.stopPropagation();
  (event.currentTarget as HTMLElement).focus();
  openParticipantMenu({ x: event.clientX, y: event.clientY }, userId, participant);
}

export function participantMenuKey(event: KeyboardEvent, userId: string, participant?: VoiceParticipant) {
  if (event.target !== event.currentTarget || !(event.key === 'ContextMenu' || (event.shiftKey && event.key === 'F10'))) return;
  event.preventDefault();
  event.stopPropagation();
  openParticipantMenu(event.currentTarget as HTMLElement, userId, participant);
}
