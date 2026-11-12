import { For, Show } from 'solid-js';
import { Button } from '../../ui';
import { channels } from '../../stores/chatStore';
import { acceptCall, dismissCall, pendingCalls } from '../../stores/flockStore';
import { t } from '../../i18n';

/** Cards for "Позвать за собой" / "Позвать сюда", bottom-left. Each closes after 30 s or when answered. */
export function FlockCallCards() {
  const roomName = (channelId: string) => channels().find((channel) => channel.id === channelId)?.name ?? '';
  return (
    <Show when={pendingCalls().length > 0}>
      <div class="flock-stack" role="region" aria-label={t('Calls')}>
        <For each={pendingCalls()}>{(call) => (
          <div class="flock-card" role="status">
            <span class="flock-text">{t('{name} is calling you to {room}', { name: call.from_name, room: roomName(call.channel_id) })}</span>
            <div class="row g8">
              <Button size="sm" variant="primary" onClick={() => void acceptCall(call.id)}>{t('Fly')}</Button>
              <Button size="sm" variant="ghost" onClick={() => dismissCall(call.id)}>{t('Stay')}</Button>
            </div>
          </div>
        )}</For>
      </div>
    </Show>
  );
}
