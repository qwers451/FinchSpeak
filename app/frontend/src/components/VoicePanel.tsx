import { createMemo, createSignal, For, Show } from 'solid-js';
import { endedShare, watching } from '../stores/screenStore';
import { localPeerId } from '../stores/sessionStore';
import { activeVoiceChannel, pingRtt, speakingPeers, voiceParticipants } from '../stores/voiceStore';
import { Avatar, Icon } from '../ui';
import { SharePickerModal } from './screen/SharePickerModal';
import { SharerPill } from './screen/SharerPill';
import { EndedPanel, ViewerPanel } from './screen/ViewerPanel';
import { Dock } from './voice/Dock';
import { LiveCard } from './voice/LiveCard';
import { isMeParticipant, ParticipantTile } from './voice/ParticipantTile';
import { VoiceSettingsModal } from './voice/VoiceSettingsModal';
import { arrange, avatarSizeFor, gridColumns, gridRows } from './voice/voiceLayout';
import { t } from '../i18n';

/** What the round-trip time to the server says about the link. */
function pingLabel(ms: number | null): { text: string; color: string } | null {
  if (ms === null) return null;
  const quality = ms < 60 ? 'excellent' : ms < 120 ? 'good' : ms < 250 ? 'fair' : 'poor';
  return { text: t('{count} ms · {quality}', { count: ms, quality: t(quality) }), color: ms < 120 ? 'var(--teal)' : 'var(--amber)' };
}

/** D1/D2: the voice room: LIVE cards, the grid of people, the roster, the dock. */
export function VoicePanel() {
  const [picker, setPicker] = createSignal(false);
  const [settings, setSettings] = createSignal(false);
  const arranged = createMemo(() => arrange(voiceParticipants(), localPeerId(), speakingPeers()));
  const tiles = () => arranged().grid.length;
  const ping = () => pingLabel(pingRtt());
  const showViewer = () => !!watching() || !!endedShare();

  return (
    <main class="main voice-room">
      <div class="mhead">
        <Icon name="vol" size="lg" class="t3" />
        <h3 style={{ 'font-size': '18px' }}>{activeVoiceChannel()?.name ?? t('Voice room')}</h3>
        <span class="tag violet">{t('{count} in room', { count: voiceParticipants().length })}</span>
        <span class="grow" />
        <Show when={ping()}>
          {(p) => <span class="pill" style={{ 'border-color': p().color, color: p().color }}><Icon name="bars" size="sm" />{p().text}</span>}
        </Show>
      </div>

      <SharerPill />

      <div class="room-body">
        <Show when={showViewer()}>
          <div class="viewer-slot"><ViewerPanel /><EndedPanel /></div>
        </Show>

        <Show when={arranged().shares.filter((share) => share.peer_id !== watching()?.peer_id).length > 0}>
          <div class="live-row">
            <For each={arranged().shares.filter((share) => share.peer_id !== watching()?.peer_id)}>{(share) => <LiveCard participant={share} />}</For>
          </div>
        </Show>

        <Show when={tiles() > 0} fallback={<div class="col center grow t2">{t('Nobody is here yet.')}</div>}>
          <div
            class={`room-grid${showViewer() ? ' compact' : ''}`}
            style={{ '--rows': showViewer() ? 1 : gridRows(tiles()), 'grid-template-columns': `repeat(${showViewer() ? Math.min(6, Math.max(3, tiles())) : gridColumns(tiles())}, minmax(0, 1fr))`, 'grid-template-rows': showViewer() ? 'auto' : `repeat(${gridRows(tiles())}, minmax(0, 1fr))` }}
            role="list"
            aria-label={t('People in the room')}
          >
            <For each={arranged().grid}>
              {(participant) => (
                <ParticipantTile participant={participant} isMe={isMeParticipant(participant, localPeerId())} avatarSize={showViewer() ? 56 : avatarSizeFor(tiles())} />
              )}
            </For>
          </div>
        </Show>

        <Show when={arranged().roster.length > 0}>
          <div class="roster" aria-label={t('More people in the room')}>
            <span class="sec-label">{t('+ {count} more · scroll', { count: arranged().roster.length })}</span>
            <div class="roster-list">
              <For each={arranged().roster}>
                {(participant) => (
                  <div class="mem">
                    <Avatar name={participant.username} id={participant.user_id} size={24} speaking={speakingPeers().has(participant.peer_id)} />
                    <span class="grow trunc">{participant.username}</span>
                    <Show when={participant.is_muted}><Icon name="mic-off" size="sm" class="tr" /></Show>
                  </div>
                )}
              </For>
            </div>
          </div>
        </Show>
      </div>

      <Dock onShare={() => setPicker(true)} onSettings={() => setSettings(true)} />
      <SharePickerModal open={picker()} onClose={() => setPicker(false)} />
      <VoiceSettingsModal open={settings()} onClose={() => setSettings(false)} />
    </main>
  );
}
