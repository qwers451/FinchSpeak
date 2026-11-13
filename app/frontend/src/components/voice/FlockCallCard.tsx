import { For, Show } from 'solid-js';
import { Button } from '../../ui';
import { createPresence } from '../../ui/motion';
import { channels } from '../../stores/chatStore';
import { acceptCall, dismissCall, pendingCalls } from '../../stores/flockStore';
import { t } from '../../i18n';

/** Cards for "Позвать за собой" / "Позвать сюда", bottom-left. Each closes after 30 s or when answered. */
export function FlockCallCards() {
  // One stable entry per call: a card that is re-rendered keeps its node and does not re-enter.
  const calls = createPresence(pendingCalls, (call) => String(call.id), 0);
  const roomName = (channelId: string) => channels().find((channel) => channel.id === channelId)?.name ?? '';
  return (
    <Show when={pendingCalls().length > 0}>
      <div class="flock-stack" role="region" aria-label={t('Calls')}>
        <For each={calls()}>{(entry) => (
          <div class={`flock-card${entry.entering ? ' flock-enter' : ''}`} role="status">
            <span class="flock-text">{t('{name} is calling you to {room}', { name: entry.item().from_name, room: roomName(entry.item().channel_id) })}</span>
            <div class="row g8">
              <Button size="sm" variant="primary" onClick={() => void acceptCall(entry.item().id)}>{t('Fly')}</Button>
              <Button size="sm" variant="ghost" onClick={() => dismissCall(entry.item().id)}>{t('Stay')}</Button>
            </div>
            {/* The call closes by itself after FLOCK_CALL_TTL_MS (30 s): this strip empties over that time. */}
            <span class="flock-timer" aria-hidden="true" />
          </div>
        )}</For>
      </div>
    </Show>
  );
}
