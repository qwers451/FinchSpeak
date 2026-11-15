import { createSignal, For, Show, type JSX } from 'solid-js';
import { participantsForChannel, speakingPeers, activeVoiceChannel, joinVoice, isMuted, isLocalSpeaking } from '../../stores/voiceStore';
import { activeChannel, selectChannel } from '../../stores/chatStore';
import { localPeerId, knownUsers } from '../../stores/sessionStore';
import { nestLevel } from '../../stores/nestView';
import { moveVoice } from '../../stores/adminStore';
import { isLocallyMuted } from '../../stores/localMuteStore';
import { gameOf } from '../../stores/activityGameStore';
import { isMeParticipant } from '../voice/ParticipantTile';
import { participantContextMenu, participantMenuKey } from '../voice/participantMenu';
import { VOICE_USER_DRAG, activeVoiceDrag, canDrag, dropTarget, readVoiceDrag, setActiveVoiceDrag } from '../voice/voiceDrag';
import { Avatar, Button, Icon } from '../../ui';
import { t } from '../../i18n';
import type { Channel, VoiceParticipant } from '../../types';

const isSpeaking = (p: VoiceParticipant) => speakingPeers().has(p.peer_id) || (isMeParticipant(p, localPeerId()) && !isMuted() && isLocalSpeaking());
const avatarOf = (p: VoiceParticipant) => knownUsers().find((user) => user.id === p.user_id)?.avatar_url;

const TWIGS = ['M18 39 Q60 53 102 39', 'M23 46 Q60 59 97 46', 'M31 53 Q60 63 89 53', 'M30 37 L44 57', 'M52 43 L40 58', 'M70 43 L82 58', 'M92 37 L78 57', 'M60 45 L61 60', 'M14 32 L5 27', 'M16 41 L6 43', 'M106 32 L115 27', 'M104 42 L114 45', 'M25 53 L17 59', 'M95 53 L104 58'];

/** The nest drawing: a hollow, optional birds peeking out, and the woven front over them. */
export function NestArt(props: { channel: Channel; width: number; birds?: boolean }) {
  const people = () => participantsForChannel(props.channel.id);
  const size = (): 24 | 32 | 40 => (props.width >= 170 ? 40 : props.width >= 120 ? 32 : 24);
  return (
    <span class="nest-art" style={{ width: `${props.width}px` }}>
      <svg class="nest-back" viewBox="0 0 120 62" aria-hidden="true"><ellipse class="nest-hollow" cx="60" cy="32" rx="46" ry="9" /></svg>
      <Show when={props.birds}>
        <span class="nest-birds">
          <For each={people().slice(0, 4)}>
            {(p) => <span class="nest-bird"><Avatar name={p.username} id={p.user_id} size={size()} src={avatarOf(p)} speaking={isSpeaking(p)} /></span>}
          </For>
        </span>
      </Show>
      <svg class="nest-front" viewBox="0 0 120 62" aria-hidden="true">
        <path class="nest-body" d="M14 32 Q60 46 106 32 Q104 60 60 60 Q16 60 14 32 Z" />
        <g class="nest-twigs"><For each={TWIGS}>{(d) => <path d={d} />}</For></g>
        <path class="nest-rim" d="M14 32 Q60 46 106 32" />
      </svg>
    </span>
  );
}

/** LIVE, the speaking equaliser and the head count, in that order. */
export function NestMarks(props: { channel: Channel }) {
  const people = () => participantsForChannel(props.channel.id);
  return (
    <>
      <Show when={people().some((participant) => participant.is_sharing)}><span class="tag live">{t('LIVE')}</span></Show>
      <Show when={people().some(isSpeaking)}><span class="nest-eq" aria-hidden="true"><i /><i /><i /></span></Show>
      <Show when={people().length > 0}><span class="nest-count">{people().length}</span></Show>
    </>
  );
}

const nestClass = (channel: Channel, extra = '') => {
  const people = participantsForChannel(channel.id);
  return `nest${people.length > 0 ? ' lit' : ''}${activeVoiceChannel()?.id === channel.id ? ' on' : ''}${extra}`;
};

/**
 * A voice room on the trunk: its name and every bird inside, by name. One click looks inside, a double click (or the
 * button in the room view) flies in, so nobody lands in a room by accident.
 */
export function NestRow(props: { channel: Channel; actions?: (channel: Channel) => JSX.Element }) {
  const [dropping, setDropping] = createSignal(false);
  const people = () => participantsForChannel(props.channel.id);
  const mine = () => activeVoiceChannel()?.id === props.channel.id;
  const selected = () => activeChannel()?.id === props.channel.id;
  const look = () => void selectChannel(props.channel);
  const join = () => { void selectChannel(props.channel); if (!mine()) void joinVoice(props.channel); };
  const dragStart = (event: DragEvent, participant: VoiceParticipant) => {
    event.stopPropagation();
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
  const rowClass = () => 'nrow ' + nestClass(props.channel, (selected() ? ' sel' : '') + (dropping() ? ' drop-target' : ''));
  const label = () => {
    const names = people().map((participant) => participant.username).join(', ');
    return `${props.channel.name}: ${names || t('empty')}`;
  };
  return (
    <div
      class={rowClass()}
      role="button"
      tabindex="0"
      data-level={nestLevel(people().length, people().some(isSpeaking))}
      aria-current={selected() ? 'page' : undefined}
      aria-label={label()}
      title={t('One click looks inside, a double click flies in')}
      onClick={look}
      onDblClick={join}
      onKeyDown={(event) => { if (event.target !== event.currentTarget) return; if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); look(); } }}
      onDragOver={dragOver}
      onDragLeave={(event) => { if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setDropping(false); }}
      onDrop={drop}
    >
      <div class="nhead">
        <NestArt channel={props.channel} width={34} />
        <span class="trunc grow" title={props.channel.name}>{props.channel.name}</span>
        <NestMarks channel={props.channel} />
        {props.actions?.(props.channel)}
      </div>
      <Show when={people().length > 0} fallback={<div class="nempty">{t('empty')}</div>}>
        <ul class="birds">
          <For each={people()}>
            {(participant) => (
              <li
                class={isSpeaking(participant) ? 'speak' : ''}
                tabindex="0"
                draggable={canDrag(participant)}
                onDragStart={(event) => dragStart(event, participant)}
                onDragEnd={() => setActiveVoiceDrag(null)}
                onClick={(event) => event.stopPropagation()}
                onDblClick={(event) => event.stopPropagation()}
                onContextMenu={(event) => participantContextMenu(event, participant.user_id, participant)}
                onKeyDown={(event) => participantMenuKey(event, participant.user_id, participant)}
              >
                <Avatar name={participant.username} id={participant.user_id} size={24} src={avatarOf(participant)} speaking={isSpeaking(participant)} />
                <span class="trunc grow" title={participant.username}>{participant.username}</span>
                <Show when={participant.is_sharing}><span class="tag live">{t('LIVE')}</span></Show>
                <Show when={gameOf(participant.user_id)}>{(game) => <Icon name="gamepad" size="sm" title={t('Playing {game}', { game: game() })} />}</Show>
                <Show when={!isMeParticipant(participant, localPeerId()) && isLocallyMuted(participant.user_id)}><Icon name="vol-off" size="sm" title={t('You muted this person')} /></Show>
                <Show when={participant.is_muted}><Icon name="mic-off" size="sm" class="tr" title={t('Muted')} /></Show>
              </li>
            )}
          </For>
        </ul>
      </Show>
    </div>
  );
}

/** Looking into a nest from outside: who is there and what they are doing, with one clear way in. */
export function NestPreview(props: { channel: Channel }) {
  const people = () => participantsForChannel(props.channel.id);
  const doing = (participant: VoiceParticipant) => {
    const game = gameOf(participant.user_id);
    if (isSpeaking(participant)) return t('speaking');
    if (participant.is_sharing) return t('sharing the screen');
    if (game) return t('Playing {game}', { game });
    if (participant.is_muted) return t('microphone off');
    return t('listening');
  };
  return (
    <section class="nest-preview" aria-label={props.channel.name}>
      <header class="mhead">
        <h3 class="room-title grow trunc">{props.channel.name}</h3>
        <NestMarks channel={props.channel} />
      </header>
      <div class="nest-preview-body">
        <div class={`nest-preview-hero ${nestClass(props.channel)}`} data-level={nestLevel(people().length, people().some(isSpeaking))}>
          <NestArt channel={props.channel} width={190} birds />
        </div>
        <Show when={people().length > 0} fallback={<p class="nest-hint">{t('Nobody is in the nest yet. Fly in first: the others will see you in the tree.')}</p>}>
          <div class="nest-tiles">
            <For each={people()}>
              {(participant) => (
                <div class={`nest-tile${isSpeaking(participant) ? ' speak' : ''}`}
                  onContextMenu={(event) => participantContextMenu(event, participant.user_id, participant)}>
                  <Avatar name={participant.username} id={participant.user_id} size={40} src={avatarOf(participant)} speaking={isSpeaking(participant)} />
                  <span class="col grow">
                    <b class="trunc" title={participant.username}>{participant.username}</b>
                    <span class="trunc t3 fs12">{doing(participant)}</span>
                  </span>
                  <Show when={participant.is_muted}><Icon name="mic-off" size="sm" class="tr" /></Show>
                </div>
              )}
            </For>
          </div>
        </Show>
        <p class="nest-hint">{t('You are looking from outside: nobody hears you.')}</p>
      </div>
      <footer class="nest-preview-foot">
        <Button variant="primary" icon="phones" onClick={() => void joinVoice(props.channel)}>{t('Fly into the nest')}</Button>
      </footer>
    </section>
  );
}
