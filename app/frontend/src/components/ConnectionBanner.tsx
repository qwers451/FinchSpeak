import { Show } from 'solid-js';
import { currentUser, logoutUser } from '../stores/authStore';
import { sessionEnded } from '../stores/sessionEnd';
import { api } from '../services/api';
import { connectionError, connectionStatus, reconnectInfo } from '../stores/sessionStore';
import { Button, Icon } from '../ui';
import { t } from '../i18n';
import { describeError } from '../i18n/errors';

/** Splash shown while the interface subscribes to the backend (A5). */
export function Splash() {
  return (
    <div class="splash" role="status">
      <img src="/custom/assets/brand/finchspeak-tile.svg" alt="" width="240" />
      <span class="spin" />
      <span class="t2">{t('Starting FinchSpeak…')}</span>
    </div>
  );
}

/** Connection state under the title bar (A5): reconnecting with the next try, lost, failed. */
export function ConnectionBanner() {
  const visible = () => !!currentUser() && connectionStatus() !== 'connected';
  // The generic QUIC text is already the headline ("Connection lost"); repeating it as a reason adds nothing.
  const reasonShown = () => !!connectionError() && connectionError()!.trim().toLowerCase() !== 'connection lost';
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
        <Show when={reasonShown()}><span class="t3 fs12">{describeError(connectionError())}</span></Show>
        <Show when={connectionStatus() === 'reconnecting'}>
          <Button size="sm" onClick={() => { void api.reconnectNow(); }}>{t('Retry now')}</Button>
        </Show>
        <Show when={connectionStatus() === 'failed' && !sessionEnded()}>
          <Button size="sm" onClick={() => { void logoutUser(); }}>{t('Back to sign in')}</Button>
        </Show>
      </div>
    </Show>
  );
}
