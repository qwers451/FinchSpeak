import { createMemo, Show } from 'solid-js';
import { currentUser } from '../../stores/authStore';
import { knownUsers } from '../../stores/sessionStore';
import { isLocallyMuted } from '../../stores/localMuteStore';
import { setVolume, volumeFor } from '../../stores/volumeStore';
import { isDeafened, isLocalSpeaking, isMuted, speakingPeers, voiceQuality } from '../../stores/voiceStore';
import type { VoiceParticipant } from '../../types';
import { openParticipantMenu, participantContextMenu, participantMenuKey } from './participantMenu';
import { Avatar, Button, Icon, Slider } from '../../ui';
import { t } from '../../i18n';

type Props = {
  participant: VoiceParticipant;
  isMe: boolean;
  avatarSize: 128 | 96 | 72 | 56;
};

/** Four bars: how good the link to this person is. */
function Quality(props: { peerId: number }) {
  const level = () => voiceQuality()[props.peerId]?.quality ?? 'good';
  const lit = () => (level() === 'good' ? 4 : level() === 'fair' ? 2 : 1);
  return (
    <div class="q" title={t(level() === 'good' ? 'Good connection' : level() === 'fair' ? 'Fair connection' : 'Poor connection')}>
      {[5, 8, 11, 14].map((height, index) => (
        <i style={{ height: `${height}px`, background: index < lit() ? (level() === 'good' ? 'var(--teal)' : 'var(--amber)') : 'var(--line)' }} />
      ))}
    </div>
  );
}

export function ParticipantTile(props: Props) {
  const p = () => props.participant;
  const speaking = () => (props.isMe ? isLocalSpeaking() : speakingPeers().has(p().peer_id));
  const muted = () => (props.isMe ? isMuted() : p().is_muted);
  const deafened = () => (props.isMe ? isDeafened() : p().is_deafened);
  const user = createMemo(() => knownUsers().find((candidate) => candidate.id === p().user_id));
  const name = () => user()?.display_name || p().username;
  const volume = () => volumeFor(p().user_id);
  const poor = () => !props.isMe && voiceQuality()[p().peer_id]?.quality === 'poor';

  return (
    <div class={`vc${speaking() ? ' speaking' : ''}`} data-peer-id={p().peer_id} tabindex="0"
      onContextMenu={(event) => participantContextMenu(event, p().user_id, p())}
      onKeyDown={(event) => participantMenuKey(event, p().user_id, p())}
      aria-label={`${name()}${muted() ? `, ${t('Muted').toLowerCase()}` : ''}${deafened() ? `, ${t('Deafened').toLowerCase()}` : ''}`}>
      <Button iconOnly size="sm" variant="ghost" icon="more" class="participant-actions" label={t('Actions for {name}', { name: name() })}
        onClick={(event) => openParticipantMenu(event.currentTarget, p().user_id, p())} />
      <Show when={!props.isMe}><Quality peerId={p().peer_id} /></Show>
      <div class="badges">
      <Show when={p().server_muted}><span title={t('Silenced by a moderator')} style={{ color: 'var(--amber)' }}><Icon name="shield" size="sm" /></span></Show>
      <Show when={!props.isMe && isLocallyMuted(p().user_id)}><span title={t('You muted this person')}><Icon name="vol-off" size="sm" /></span></Show>
      <Show when={muted()}><span title={t('Muted')}><Icon name="mic-off" size="sm" /></span></Show>
      <Show when={deafened()}><span title={t('Deafened')}><Icon name="phones-off" size="sm" /></span></Show>
      </div>
      <Avatar name={name()} id={p().user_id} size={props.isMe ? Math.max(props.avatarSize, 72) as 128 | 96 | 72 | 56 : props.avatarSize} src={user()?.avatar_url} speaking={speaking()} levelPeerId={p().peer_id} />
      <span class="nm head trunc">
        {name()}
        <Show when={props.isMe}> <span class="t3 fs12" style={{ 'font-family': 'var(--font-ui)' }}>{t('(you)')}</span></Show>
        <Show when={user()?.role === 'admin'}> <span class="tag admin">{t('Admin')}</span></Show>
      </span>
      <Show when={speaking()}><span class="tag cyan">{t('Speaking')}</span></Show>
      <Show when={poor()}><span class="tag amber">{t('Poor connection')}</span></Show>
      <Show when={!props.isMe}>
        <div class="vol">
          <Icon name={volume() === 0 ? 'vol-off' : 'vol'} size="sm" />
          <Slider cool label={t('Volume of {name}', { name: name() })} min={0} max={2} step={0.05} value={volume()} disabled={isLocallyMuted(p().user_id)}
            onChange={(value) => void setVolume(p().user_id, p().peer_id, value)} />
          <span class="fs12 t2 vol-num">{Math.round(volume() * 100)}%</span>
        </div>
      </Show>
    </div>
  );
}

export function isMeParticipant(participant: VoiceParticipant, localPeerId: number | null) {
  return participant.peer_id === localPeerId || (localPeerId === null && participant.user_id === currentUser()?.id);
}
