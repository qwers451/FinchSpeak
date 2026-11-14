import { createSignal, Show } from 'solid-js';
import { activeVoiceChannel } from '../../stores/voiceStore';
import { isSharing, stopSharing } from '../../stores/screenStore';
import { musicAvailable, musicPanelOpen, musicPosition, musicState, sendMusicCommand, setMusicPanelOpen } from '../../stores/musicStore';
import { setSoundboardOpen, soundboardAvailable } from '../../stores/soundboardStore';
import { Button, Icon, Modal } from '../../ui';
import { MusicPanel, formatClock } from '../music/MusicPanel';
import { SoundboardPanel } from './SoundboardPanel';
import { SharePickerModal } from '../screen/SharePickerModal';
import { t } from '../../i18n';

/** "In the room": what plays now, and the Music, Sounds and Screen buttons. Shown only while I am in a voice room. */
export function RoomTools(props: { compact?: boolean }) {
  const [picker, setPicker] = createSignal(false);
  return (
    <Show when={activeVoiceChannel()}>
      <section class={`room-tools${props.compact ? ' compact' : ''}`} aria-label={t('In the room')}>
        <Show when={musicAvailable() && musicState()?.playing}>
          {(now) => (
            <div class="room-now">
              <div class="room-now-title">
                <Icon name="music" size="sm" class="t3" />
                <span class="trunc grow" title={now().entry.track.title}>{now().entry.track.title}</span>
                <span class="t3 fs12 music-clock">{formatClock(musicPosition())}</span>
              </div>
              <div class="room-now-actions">
                <Button size="sm" variant="secondary" icon={now().paused ? 'resume' : 'pause'} aria-label={now().paused ? t('Resume') : t('Pause')} title={now().paused ? t('Resume') : t('Pause')} onClick={() => void sendMusicCommand({ kind: now().paused ? 'resume' : 'pause' })}>
                  <span class="rt-label">{now().paused ? t('Resume') : t('Pause')}</span>
                </Button>
                <Button size="sm" variant="secondary" icon="next" aria-label={t('Next')} title={t('Next')} onClick={() => void sendMusicCommand({ kind: 'skip' })}>
                  <span class="rt-label">{t('Next')}</span>
                </Button>
              </div>
            </div>
          )}
        </Show>
        <div class="room-tools-buttons">
          <Show when={musicAvailable()}>
            <Button size="sm" variant="secondary" icon="music" aria-label={t('Music')} title={t('Open the player')} aria-pressed={musicPanelOpen()} onClick={() => setMusicPanelOpen(true)}>
              <span class="rt-label">{t('Music')}</span>
            </Button>
          </Show>
          <Show when={soundboardAvailable()}>
            <Button size="sm" variant="secondary" icon="smile" aria-label={t('Sounds')} title={t('Open the soundboard')} onClick={() => setSoundboardOpen(true)}>
              <span class="rt-label">{t('Sounds')}</span>
            </Button>
          </Show>
          <Button size="sm" variant="secondary" icon="share" class={isSharing() ? 'live' : ''} aria-label={t('Screen')} aria-pressed={isSharing()} title={t(isSharing() ? 'Stop sharing' : 'Share your screen')} onClick={() => (isSharing() ? void stopSharing() : setPicker(true))}>
            <span class="rt-label">{t('Screen')}</span>
          </Button>
        </div>
        <SharePickerModal open={picker()} onClose={() => setPicker(false)} />
      </section>
    </Show>
  );
}

/** The player and the soundboard as app-level windows: they open from any screen while I am in a room. */
export function RoomToolWindows() {
  const close = () => setMusicPanelOpen(false);
  return (
    <>
      <Modal open={musicAvailable() && musicPanelOpen()} onClose={close} title={t('Music')} width="min(480px, 100%)">
        <MusicPanel modal onClose={close} />
      </Modal>
      <Show when={soundboardAvailable()}><SoundboardPanel /></Show>
    </>
  );
}
