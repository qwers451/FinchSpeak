import { createMemo, createSignal, For, Show } from 'solid-js';
import { commitSeats, currentLayout, dragSeat, placeSeat, resetSeats, seats } from '../../stores/spatialStore';
import { speakingPeers } from '../../stores/voiceStore';
import { Avatar, Button, Icon } from '../../ui';
import { t } from '../../i18n';

/** How far a voice is placed from the centre, as a share of half the stage width. */
const REACH = 0.44;
const STEP = 0.1;

/** Left/right position on the stage for a pan, in percent. */
export const seatLeft = (pan: number) => `${50 + pan * REACH * 100}%`;
/** Voices at the sides sit a little lower, so the row reads as an arc around you. */
export const seatTop = (pan: number) => `${10 + Math.abs(pan) ** 1.6 * 30}px`;

/** Drag a person left or right to hear them from there; you sit in the middle. */
export function SpatialStage() {
  let stage: HTMLDivElement | undefined;
  const [dragging, setDragging] = createSignal<string | null>(null);
  const layout = createMemo(currentLayout);
  // Keyed by the participant (a stable object), so a drag does not rebuild the button it holds.
  const people = createMemo(() => layout().map((seat) => seat.participant));
  const seatOf = (userId: string) => layout().find((seat) => seat.participant.user_id === userId) ?? { pan: 0, pinned: false };

  const panAt = (clientX: number) => {
    if (!stage) return 0;
    const box = stage.getBoundingClientRect();
    return (clientX - (box.left + box.width / 2)) / (box.width * REACH);
  };

  return (
    <section class="spatial-stage" aria-label={t('Spatial audio: where everyone sounds from')}>
      <div class="row between spatial-head">
        <span class="sec-label"><Icon name="phones" size="sm" />{t('Spatial audio')}</span>
        <Show when={Object.keys(seats()).length > 0}>
          <Button size="sm" variant="ghost" icon="refresh" onClick={() => resetSeats()}>{t('Spread out again')}</Button>
        </Show>
      </div>
      <div class="stage-area" ref={stage} classList={{ dragging: dragging() !== null }}>
        <span class="stage-side l" aria-hidden="true">{t('L')}</span>
        <span class="stage-side r" aria-hidden="true">{t('R')}</span>
        <For each={people()}>
          {(participant) => {
            const seat = () => ({ participant, ...seatOf(participant.user_id) });
            return (
            <button
              type="button"
              class="seat"
              classList={{ moving: dragging() === seat().participant.user_id, pinned: seat().pinned }}
              style={{ left: seatLeft(seat().pan), top: seatTop(seat().pan) }}
              aria-label={t('{name}: drag or use the arrow keys to move left or right', { name: seat().participant.username })}
              aria-valuetext={seat().pan < -0.05 ? t('left') : seat().pan > 0.05 ? t('right') : t('centre')}
              title={t('Double-click to put back')}
              onPointerDown={(event) => {
                event.currentTarget.setPointerCapture(event.pointerId);
                setDragging(seat().participant.user_id);
              }}
              onPointerMove={(event) => {
                if (dragging() === seat().participant.user_id) dragSeat(seat().participant.user_id, panAt(event.clientX));
              }}
              onPointerUp={() => { setDragging(null); commitSeats(); }}
              onPointerCancel={() => { setDragging(null); commitSeats(); }}
              onDblClick={() => resetSeats(seat().participant.user_id)}
              onKeyDown={(event) => {
                if (event.key === 'ArrowLeft' || event.key === 'ArrowRight') {
                  event.preventDefault();
                  placeSeat(seat().participant.user_id, seat().pan + (event.key === 'ArrowLeft' ? -STEP : STEP));
                }
              }}
            >
              <Avatar name={seat().participant.username} id={seat().participant.user_id} size={24} speaking={speakingPeers().has(seat().participant.peer_id)} />
              <span class="seat-name trunc">{seat().participant.username}</span>
            </button>
            );
          }}
        </For>
        <span class="listener" aria-hidden="true"><Icon name="phones" size="sm" />{t('you')}</span>
        <Show when={layout().length === 0}>
          <span class="stage-empty t3 fs12">{t('When someone joins, you will hear them from their place here.')}</span>
        </Show>
      </div>
    </section>
  );
}
