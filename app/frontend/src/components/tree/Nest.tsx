import { createSignal, For, Show, type JSX } from 'solid-js';
import { createPresence } from '../../ui/motion';
import { participantsForChannel, speakingPeers, activeVoiceChannel, joinVoice, isMuted, isLocalSpeaking } from '../../stores/voiceStore';
import { selectChannel } from '../../stores/chatStore';
import { localPeerId, knownUsers } from '../../stores/sessionStore';
import { nestLevel } from '../../stores/nestView';
import { moveVoice } from '../../stores/adminStore';
import { isLocallyMuted } from '../../stores/localMuteStore';
import { gameOf } from '../../stores/activityGameStore';
import { isMeParticipant } from '../voice/ParticipantTile';
import { participantContextMenu, participantMenuKey } from '../voice/participantMenu';
import { VOICE_USER_DRAG, activeVoiceDrag, canDrag, dropTarget, readVoiceDrag, setActiveVoiceDrag } from '../voice/voiceDrag';
import { Avatar, Icon } from '../../ui';
import { t } from '../../i18n';
import type { Channel, VoiceParticipant } from '../../types';

const VISIBLE = 3;

const isSpeaking = (p: VoiceParticipant) => speakingPeers().has(p.peer_id) || (isMeParticipant(p, localPeerId()) && !isMuted() && isLocalSpeaking());

const TWIGS = ['M18 39 Q60 53 102 39', 'M23 46 Q60 59 97 46', 'M31 53 Q60 63 89 53', 'M30 37 L44 57', 'M52 43 L40 58', 'M70 43 L82 58', 'M92 37 L78 57', 'M60 45 L61 60', 'M14 32 L5 27', 'M16 41 L6 43', 'M106 32 L115 27', 'M104 42 L114 45', 'M25 53 L17 59', 'M95 53 L104 58'];

/** The woven front of the nest: it covers the lower part of the birds so they peek out from behind the rim. */
function NestFront() {
  return (
    <svg class="nest-front" viewBox="0 0 120 62" aria-hidden="true">
      <path class="nest-body" d="M14 32 Q60 46 106 32 Q104 60 60 60 Q16 60 14 32 Z" />
      <g class="nest-twigs"><For each={TWIGS}>{(d) => <path d={d} />}</For></g>
      <path class="nest-rim" d="M14 32 Q60 46 106 32" />
    </svg>
  );
}

/** A voice room as a nest: its birds sit inside, an empty nest stays dim, a busy one warms up. */
export function Nest(props: { channel: Channel; actions?: (channel: Channel) => JSX.Element }) {
  const [dropping, setDropping] = createSignal(false);
  const people = () => participantsForChannel(props.channel.id);
  // One stable entry per visible bird: a bird whose data changed keeps its node; a newcomer hops in.
  const birds = createPresence(() => people().slice(0, VISIBLE), (participant) => participant.user_id, 0);
  const someoneSpeaking = () => people().some(isSpeaking);
  const mine = () => activeVoiceChannel()?.id === props.channel.id;
  const join = () => { void selectChannel(props.channel); if (!mine()) void joinVoice(props.channel); };
  const dragStart = (event: DragEvent, participant: VoiceParticipant) => {
    if (!canDrag(participant) || !event.dataTransfer) { event.preventDefault(); return; }
    const payload = { userId: participant.user_id, peerId: participant.peer_id, from: participant.channel_id };
    event.dataTransfer.setData(VOICE_USER_DRAG, JSON.stringify(payload));
    event.dataTransfer.effectAllowed = 'move';
    setActiveVoiceDrag(payload);
  };
  const dragOver = (event: DragEvent) => {
    if (!event.dataTransfer?.types.includes(VOICE_USER_DRAG) || event.dataTransfer.types.includes('Files') || !dropTarget(activeVoiceDrag(), props.channel)) return;
    event.preventDefault();
    event.dataTransfer.dropEffect = 'move';
    setDropping(true);
  };
  const drop = (event: DragEvent) => {
    const participant = dropTarget(readVoiceDrag(event.dataTransfer), props.channel);
    setDropping(false);
    setActiveVoiceDrag(null);
    if (!participant) return;
    event.preventDefault();
    event.stopPropagation();
    if (isMeParticipant(participant, localPeerId())) void joinVoice(props.channel);
    else void moveVoice(participant.user_id, participant.peer_id, props.channel.id);
  };
  return (
    <div
      class={`nest${people().length > 0 ? ' lit' : ''}${mine() ? ' on' : ''}${dropping() ? ' drop-target' : ''}`}
      role="button"
      tabindex="0"
      data-level={nestLevel(people().length, someoneSpeaking())}
      aria-label={people().length > 0 ? `${props.channel.name}, ${people().length}` : `${props.channel.name}, ${t('Empty nest')}`}
      onClick={join}
      onKeyDown={(event) => { if (event.target !== event.currentTarget) return; if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); join(); } }}
      onDragOver={dragOver}
      onDragLeave={(event) => { if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setDropping(false); }}
      onDrop={drop}
    >
      <div class="nest-top">
        <span class="trunc" title={props.channel.name}>{props.channel.name}</span>
        <Show when={people().some((participant) => participant.is_sharing)}><span class="tag live">{t('LIVE')}</span></Show>
        <Show when={someoneSpeaking()}><span class="nest-eq" aria-hidden="true"><i /><i /><i /></span></Show>
        <Show when={people().length > 0}><span class="nest-count">{people().length}</span></Show>
        {props.actions?.(props.channel)}
      </div>
      <div class="nest-art">
        <svg class="nest-back" viewBox="0 0 120 62" aria-hidden="true"><ellipse class="nest-hollow" cx="60" cy="32" rx="46" ry="9" /></svg>
        <div class="nest-birds">
          <For each={birds()}>
            {(bird) => { const participant = () => bird.item(); return (
              <span
                class={`nest-bird${bird.entering ? ' nest-bird-enter' : ''}`}
                title={participant().username}
                tabindex="0"
                draggable={canDrag(participant())}
                onDragStart={(event) => dragStart(event, participant())}
                onDragEnd={() => setActiveVoiceDrag(null)}
                onContextMenu={(event) => participantContextMenu(event, participant().user_id, participant())}
                onKeyDown={(event) => participantMenuKey(event, participant().user_id, participant())}
              >
                <Avatar name={participant().username} id={participant().user_id} size={32} src={knownUsers().find((user) => user.id === participant().user_id)?.avatar_url} speaking={isSpeaking(participant())} />
                <Show when={participant().is_muted}><Icon name="mic-off" size="sm" class="nest-badge nest-muted" title={t('Muted')} /></Show>
                <Show when={!isMeParticipant(participant(), localPeerId()) && isLocallyMuted(participant().user_id)}>
                  <Icon name="vol-off" size="sm" class="nest-badge nest-local-mute" title={t('You muted this person')} />
                </Show>
                <Show when={gameOf(participant().user_id)}>
                  {(game) => <Icon name="gamepad" size="sm" class="nest-badge nest-game" title={t('Playing {game}', { game: game() })} />}
                </Show>
              </span>
            ); }}
          </For>
          <Show when={people().length > VISIBLE}><span class="nest-more">+{people().length - VISIBLE}</span></Show>
        </div>
        <NestFront />
      </div>
    </div>
  );
}
