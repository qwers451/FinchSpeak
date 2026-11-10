import { For, Show, type JSX } from 'solid-js';
import { participantsForChannel, speakingPeers, activeVoiceChannel, joinVoice, isMuted, isLocalSpeaking } from '../../stores/voiceStore';
import { selectChannel } from '../../stores/chatStore';
import { localPeerId, knownUsers } from '../../stores/sessionStore';
import { nestLevel } from '../../stores/nestView';
import { isMeParticipant } from './ParticipantTile';
import { Avatar, Icon } from '../../ui';
import { t } from '../../i18n';
import type { Channel, VoiceParticipant } from '../../types';

const VISIBLE = 3;

const isSpeaking = (p: VoiceParticipant) => speakingPeers().has(p.peer_id) || (isMeParticipant(p, localPeerId()) && !isMuted() && isLocalSpeaking());

const TWIGS = ['M18 39 Q60 53 102 39', 'M23 46 Q60 59 97 46', 'M31 53 Q60 63 89 53', 'M30 37 L44 57', 'M52 43 L40 58', 'M70 43 L82 58', 'M92 37 L78 57', 'M60 45 L61 60', 'M14 32 L5 27', 'M16 41 L6 43', 'M106 32 L115 27', 'M104 42 L114 45', 'M25 53 L17 59', 'M95 53 L104 58'];

/** The woven front of the nest: it covers the lower part of the birds so they peek out from behind the rim. */
function NestFront() {
  return (
    <svg class="nest-front" viewBox="0 0 120 72" aria-hidden="true">
      <path class="nest-body" d="M14 32 Q60 46 106 32 Q104 60 60 60 Q16 60 14 32 Z" />
      <g class="nest-twigs"><For each={TWIGS}>{(d) => <path d={d} />}</For></g>
      <path class="nest-rim" d="M14 32 Q60 46 106 32" />
      <path class="nest-branch" d="M2 65 Q60 70 118 64" />
      <path class="nest-leaf" d="M100 64 q7 -11 15 -5 q-5 7 -15 5z" />
    </svg>
  );
}

/** Voice channels as nests on a branch: birds sit inside, an empty nest stays dim. */
export function NestMap(props: { channels: Channel[]; actions?: (channel: Channel) => JSX.Element }) {
  return (
    <div class="nest-map" role="list">
      <div class="nest-grid">
        <For each={props.channels}>
          {(channel) => {
            const people = () => participantsForChannel(channel.id);
            const someoneSpeaking = () => people().some(isSpeaking);
            const level = () => nestLevel(people().length, someoneSpeaking());
            const join = () => { void selectChannel(channel); if (activeVoiceChannel()?.id !== channel.id) void joinVoice(channel); };
            return (
              <div
                class={`nest${people().length > 0 ? ' lit' : ''}${activeVoiceChannel()?.id === channel.id ? ' on' : ''}`}
                role="button"
                tabindex="0"
                data-level={level()}
                aria-label={people().length > 0 ? `${channel.name}, ${people().length}` : `${channel.name}, ${t('Empty nest')}`}
                onClick={join}
                onKeyDown={(event) => { if (event.target !== event.currentTarget) return; if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); join(); } }}
              >
                <div class="nest-top">
                  <Icon name="vol" size="sm" />
                  <span class="trunc" title={channel.name}>{channel.name}</span>
                  <Show when={people().some((participant) => participant.is_sharing)}><span class="tag live">{t('LIVE')}</span></Show>
                  <Show when={someoneSpeaking()}><span class="nest-eq" aria-hidden="true"><i /><i /><i /></span></Show>
                  <Show when={people().length > 0}><span class="nest-count">{people().length}</span></Show>
                  {props.actions?.(channel)}
                </div>
                <div class="nest-art">
                  <svg class="nest-back" viewBox="0 0 120 72" aria-hidden="true"><ellipse class="nest-hollow" cx="60" cy="32" rx="46" ry="9" /></svg>
                  <div class="nest-birds">
                    <For each={people().slice(0, VISIBLE)}>
                      {(participant) => (
                        <Avatar name={participant.username} id={participant.user_id} size={24} src={knownUsers().find((user) => user.id === participant.user_id)?.avatar_url} speaking={isSpeaking(participant)} />
                      )}
                    </For>
                    <Show when={people().length > VISIBLE}><span class="nest-more">+{people().length - VISIBLE}</span></Show>
                  </div>
                  <NestFront />
                </div>
              </div>
            );
          }}
        </For>
      </div>
    </div>
  );
}
