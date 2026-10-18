import { Show } from 'solid-js';
import { watchShare, watching } from '../../stores/screenStore';
import { knownUsers } from '../../stores/sessionStore';
import type { VoiceParticipant } from '../../types';
import { Avatar, Button } from '../../ui';
import { t } from '../../i18n';

/** A wide card for someone who shares their screen, with the Watch button. */
export function LiveCard(props: { participant: VoiceParticipant }) {
  const p = () => props.participant;
  const name = () => knownUsers().find((user) => user.id === p().user_id)?.display_name || p().username;
  const watched = () => watching()?.peer_id === p().peer_id;
  return (
    <div class="vc live-card" data-peer-id={p().peer_id}>
      <div class="live-art" aria-hidden="true">
        <img src="/custom/assets/brand/finch-mark.svg" alt="" width="64" height="64" />
        <span class="tag live"><span class="dot live" style={{ background: 'var(--bg-app)', width: '6px', height: '6px' }} />{t('Live')}</span>
      </div>
      <div class="row g8 live-foot">
        <Avatar name={name()} id={p().user_id} size={32} />
        <div class="col grow" style={{ 'min-width': 0 }}>
          <strong class="head fs16 trunc">{name()}</strong>
          <span class="t2 fs12">{t('is sharing a screen')}</span>
        </div>
        <Button variant={watched() ? 'secondary' : 'primary'} icon={watched() ? undefined : 'eye'} disabled={watched()} onClick={() => void watchShare(p().user_id, p().peer_id)}>
          <Show when={watched()} fallback={t('Watch')}>{t('Watching')}</Show>
        </Button>
      </div>
    </div>
  );
}
