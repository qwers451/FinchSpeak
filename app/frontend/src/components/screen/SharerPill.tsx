import { Show } from 'solid-js';
import { sharingInfo, stopSharing } from '../../stores/screenStore';
import { Button, Icon } from '../../ui';

/** E2: while you share, a plate at the top of the room says so and stops it. */
export function SharerPill() {
  return (
    <Show when={sharingInfo()}>
      {(info) => (
        <div class="sharer-pill" role="status" aria-live="polite">
          <span class="dot live" aria-hidden="true" />
          <div class="col grow" style={{ 'min-width': 0 }}>
            <strong class="fs13 trunc">You are sharing {info().label || 'your screen'}</strong>
            <span class="t2 fs12">{info().audio ? 'With system sound · 30 fps' : 'Without sound · 30 fps'}</span>
          </div>
          <Icon name={info().audio ? 'vol' : 'vol-off'} size="sm" class="t2" title={info().audio ? 'System sound is shared' : 'System sound is not shared'} />
          <Button size="sm" variant="danger" icon="stop" onClick={() => void stopSharing()}>Stop sharing</Button>
        </div>
      )}
    </Show>
  );
}
