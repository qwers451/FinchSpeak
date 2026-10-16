import { Show } from 'solid-js';
import { currentUser } from '../stores/authStore';
import { connectionError, connectionStatus, reconnectInfo } from '../stores/sessionStore';
import { Icon } from '../ui';

/** Splash shown while the interface subscribes to the backend (A5). */
export function Splash() {
  return (
    <div class="splash" role="status">
      <img src="./mascot.svg" alt="" width="240" />
      <span class="spin" />
      <span class="t2">Starting FinchSpeak…</span>
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
              ? `Reconnecting… try ${reconnectInfo()!.attempt}, next one in ${reconnectInfo()!.retryInSecs} s`
              : 'Reconnecting…'
            : connectionStatus() === 'failed'
              ? 'Could not reconnect'
              : 'Connection lost'}
        </span>
        <Show when={connectionError()}><span class="t3 fs12">{connectionError()}</span></Show>
      </div>
    </Show>
  );
}
