import { Show } from 'solid-js';
import { screenAudioSource, sharingInfo, stopSharing } from '../../stores/screenStore';
import { Button, Icon } from '../../ui';
import { t } from '../../i18n';

/** E2: while you share, a plate at the top of the room says so and stops it. */
export function SharerPill() {
  return (
    <Show when={sharingInfo()}>
      {(info) => (
        <div class="sharer-pill" role="status" aria-live="polite">
          <span class="dot live" aria-hidden="true" />
          <div class="col grow" style={{ 'min-width': 0 }}>
            <strong class="fs13 trunc">{t('You are sharing {label}', { label: info().label || t('your screen') })}</strong>
            <span class="t2 fs12">{info().audio ? t('With system sound · 30 fps') : t('Without sound · 30 fps')}</span>
            <Show when={info().audio && screenAudioSource() === 'device'}>
              <div class="auth-banner amber" role="note">
                <span class="fs12">{t('This version of Windows cannot leave FinchSpeak out of the sound: people in the room will hear themselves with a delay. Update to Windows 10 2004 or newer.')}</span>
              </div>
            </Show>
          </div>
          <Icon name={info().audio ? 'vol' : 'vol-off'} size="sm" class="t2" title={info().audio ? t('System sound is shared') : t('System sound is not shared')} />
          <Button size="sm" variant="danger" icon="stop" onClick={() => void stopSharing()}>{t('Stop sharing')}</Button>
        </div>
      )}
    </Show>
  );
}
