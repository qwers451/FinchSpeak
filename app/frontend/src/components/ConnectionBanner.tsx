import { Show } from 'solid-js';
import { currentUser } from '../stores/authStore';
import { connectionError, connectionStatus, reconnectInfo } from '../stores/sessionStore';
import { Icon } from '../ui';
import { t } from '../i18n';

/** Splash shown while the interface subscribes to the backend (A5). */
export function Splash() {
  return (
    <div class="splash" role="status">
      <img src="/custom/assets/illustrations/mascot.svg" alt="" width="240" />
      <span class="spin" />
      <span class="t2">{t('Starting FinchSpeak…')}</span>
    </div>
  );
}

/** Connection state under the title bar (A5): reconnecting with the next try, lost, failed. */
export function ConnectionBanner() {
  const visible = () => !!currentUser() && connectionStatus() !== 'connected';
  return (
    <Show when={visible()}>
      <div class={`banner ${connectionStatus() === 'reconnecting' ? 'warn' : 'error'}`} role="status">
        <Show when={connectionStatus() === 'reconnecting'} fallback={<Icon name="warn" size="sm" />}>
          <span class="spin" />
        </Show>
        <span>
          {connectionStatus() === 'reconnecting'
            ? reconnectInfo()
              ? t('Reconnecting… try {attempt}, next one in {seconds} s', { attempt: reconnectInfo()!.attempt, seconds: reconnectInfo()!.retryInSecs })
              : t('Reconnecting…')
            : connectionStatus() === 'failed'
              ? t('Could not reconnect')
              : t('Connection lost')}
        </span>
        <Show when={connectionError()}><span class="t3 fs12">{connectionError()}</span></Show>
      </div>
    </Show>
  );
}
