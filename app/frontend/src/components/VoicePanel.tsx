import { createMemo, createSignal, For, onCleanup, onMount, Show } from 'solid-js';
import { endedShare, watching } from '../stores/screenStore';
import { localPeerId, serverFeatures } from '../stores/sessionStore';
import { activeVoiceChannel, pingRtt, speakingPeers, voiceParticipants } from '../stores/voiceStore';
import { Avatar, Button, Icon, Modal } from '../ui';
import { channels } from '../stores/chatStore';
import { callFlock } from '../stores/flockStore';
import { openContextMenu, type MenuItem } from '../ui/ContextMenu';
import { MusicPanel, formatClock } from './music/MusicPanel';
import { SoundboardPanel } from './voice/SoundboardPanel';
import { soundboardAvailable } from '../stores/soundboardStore';
import { musicAvailable, musicPanelOpen, musicPosition, musicState, setMusicPanelOpen } from '../stores/musicStore';
import { SharePickerModal } from './screen/SharePickerModal';
import { SharerPill } from './screen/SharerPill';
import { EndedPanel, ViewerPanel } from './screen/ViewerPanel';
import { Dock } from './voice/Dock';
import { LiveCard } from './voice/LiveCard';
import { isMeParticipant, ParticipantTile } from './voice/ParticipantTile';
import { VoiceSettingsModal } from './voice/VoiceSettingsModal';
import { SpatialStage } from './voice/SpatialStage';
import { spatialEnabled } from '../stores/spatialStore';
import { arrange, avatarSizeFor, gridColumns, gridRows } from './voice/voiceLayout';
import { participantContextMenu, participantMenuKey } from './voice/participantMenu';
import { tileOrder } from '../stores/tileOrderStore';
import { t } from '../i18n';

/** What the round-trip time to the server says about the link. */
function pingLabel(ms: number | null): { text: string; tone: 'good' | 'slow' } | null {
  if (ms === null) return null;
  const quality = ms < 60 ? 'excellent' : ms < 120 ? 'good' : ms < 250 ? 'fair' : 'poor';
  return { text: t('{count} ms · {quality}', { count: ms, quality: t(quality) }), tone: ms < 120 ? 'good' : 'slow' };
}

/** D1/D2: the voice room: LIVE cards, the grid of people, the roster, the dock. */
export function VoicePanel() {
  const [picker, setPicker] = createSignal(false);
  const [settings, setSettings] = createSignal(false);
  const arranged = createMemo(() => arrange(voiceParticipants(), localPeerId(), speakingPeers(), tileOrder()));
  const tiles = () => arranged().grid.length;
  const ping = () => pingLabel(pingRtt());
  const showViewer = () => !!watching() || !!endedShare();
  /** Rooms I could call everybody to: every voice room but the one I am in. */
  const otherRooms = () => channels().filter((room) => room.channel_type === 'voice' && room.id !== activeVoiceChannel()?.id);
  const openFlockMenu = (anchor: HTMLElement) => openContextMenu(anchor, otherRooms().map((room): MenuItem => ({
    kind: 'item', label: room.name, icon: 'vol', onSelect: () => void callFlock(room),
  })));
  // Below 940 px the music panel is a dialog; wider, it is a 340 px column beside the room.
  const [wide, setWide] = createSignal(window.innerWidth >= 940);
  onMount(() => {
    const onResize = () => setWide(window.innerWidth >= 940);
    window.addEventListener('resize', onResize);
    onCleanup(() => window.removeEventListener('resize', onResize));
  });
  const musicOpen = () => musicAvailable() && musicPanelOpen();
  const sidePanel = () => musicOpen() && wide();
  const closeMusic = () => setMusicPanelOpen(false);

  return (
    <main class={`main voice-room${sidePanel() ? ' music-open' : ''}`}>
      <div class="room-column">
      <div class="mhead">
        <Icon name="vol" size="lg" class="t3" />
        <h3 class="room-title">{activeVoiceChannel()?.name ?? t('Voice room')}</h3>
        <span class="tag violet">{t('{count} in room', { count: voiceParticipants().length })}</span>
        <span class="grow" />
        <Show when={serverFeatures().includes('flock')}>
          <Button size="sm" variant="ghost" icon="vol" disabled={otherRooms().length === 0} onClick={(event) => openFlockMenu(event.currentTarget)}>
            {t('Call everyone along')}
          </Button>
        </Show>
        <Show when={musicAvailable() && musicState()?.playing}>
          {(now) => (
            <button type="button" class="music-now-chip" title={t('Music')} onClick={() => setMusicPanelOpen(true)}>
              <Icon name="music" size="sm" />
              <span class="trunc">{now().entry.track.title}</span>
              <span class="t3 fs12 tabular">{formatClock(musicPosition())}</span>
            </button>
          )}
        </Show>
        <Show when={ping()}>
          {(p) => <span class={`pill ping-${p().tone}`}><Icon name="bars" size="sm" />{p().text}</span>}
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
                  <div class="mem" tabindex="0" onContextMenu={(event) => participantContextMenu(event, participant.user_id, participant)} onKeyDown={(event) => participantMenuKey(event, participant.user_id, participant)}>
                    <Avatar name={participant.username} id={participant.user_id} size={24} speaking={speakingPeers().has(participant.peer_id)} levelPeerId={participant.peer_id} />
                    <span class="grow trunc">{participant.username}</span>
                    <Show when={participant.is_muted}><Icon name="mic-off" size="sm" class="tr" /></Show>
                  </div>
                )}
              </For>
            </div>
          </div>
        </Show>
      </div>

      <Show when={spatialEnabled()}><SpatialStage /></Show>
      <Dock onShare={() => setPicker(true)} onSettings={() => setSettings(true)} />
      <SharePickerModal open={picker()} onClose={() => setPicker(false)} />
      <VoiceSettingsModal open={settings()} onClose={() => setSettings(false)} />
      </div>
      <Show when={sidePanel()}><MusicPanel onClose={closeMusic} /></Show>
      <Show when={soundboardAvailable()}><SoundboardPanel /></Show>
      <Show when={musicOpen() && !wide()}>
        <Modal open onClose={closeMusic} title={t('Music')} width="340px">
          <MusicPanel modal onClose={closeMusic} />
        </Modal>
      </Show>
    </main>
  );
}
