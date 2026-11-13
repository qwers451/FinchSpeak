import { render } from 'solid-js/web';
import { afterEach, beforeEach, expect, it } from 'vitest';
import { OverlayView } from './OverlayView';
import type { OverlayState } from '../../stores/overlayStore';
import { DEFAULT_PREFS, setPrefs } from '../../stores/settingsStore';

let dispose: (() => void) | undefined;
beforeEach(() => setPrefs({ ...DEFAULT_PREFS, appearance: { ...DEFAULT_PREFS.appearance, language: 'ru' } }));
afterEach(() => {
  dispose?.();
  dispose = undefined;
  document.body.innerHTML = '';
});

const mount = (state: OverlayState | null, opacity = 0.85) => {
  const host = document.createElement('div');
  document.body.appendChild(host);
  dispose = render(() => <OverlayView state={state} opacity={opacity} />, host);
  return host;
};

it('says that I am not in a room when there is no room', () => {
  const host = mount(null);
  expect(host.textContent).toContain('Не в комнате');
  expect(host.querySelector('.overlay-people')).toBeNull();
});

it('shows the room and each person, marks who is speaking and who is muted', () => {
  const host = mount({
    room: 'Лобби',
    me_muted: false,
    people: [
      { name: 'Alice', avatar_url: null, speaking: true, muted: false },
      { name: 'Anna', avatar_url: null, speaking: false, muted: true },
    ],
  }, 0.5);
  expect(host.querySelector('.overlay-room')?.textContent).toBe('Лобби');
  const rows = host.querySelectorAll('.overlay-person');
  expect(rows).toHaveLength(2);
  expect(rows[0].textContent).toContain('Alice');
  expect(rows[0].classList.contains('speaking')).toBe(true);
  expect(rows[1].classList.contains('speaking')).toBe(false);
  expect(rows[1].querySelector('[aria-label="Микрофон выключен"]')).not.toBeNull();
  expect(rows[0].querySelector('[aria-label="Микрофон выключен"]')).toBeNull();
  expect((host.querySelector('.overlay-card') as HTMLElement).style.opacity).toBe('0.5');
});

it('the card can be dragged from its head and room name', () => {
  const host = mount({ room: 'Лобби', me_muted: false, people: [] });
  expect(host.querySelector('.overlay-card')?.hasAttribute('data-tauri-drag-region')).toBe(true);
  expect(host.querySelector('.overlay-head')?.hasAttribute('data-tauri-drag-region')).toBe(true);
});
