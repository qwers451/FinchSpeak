import { render } from 'solid-js/web';
import { t } from '../../i18n';
import { afterEach, beforeEach, expect, it } from 'vitest';
import { setServerFeatures } from '../../stores/sessionStore';
import { Dock } from './Dock';

let dispose: (() => void) | undefined;

beforeEach(() => {
  setServerFeatures(['music', 'soundboard']);
  const root = document.createElement('div');
  document.body.append(root);
  dispose = render(() => <Dock onShare={() => {}} onSettings={() => {}} />, root);
});

afterEach(() => {
  dispose?.();
  setServerFeatures([]);
  document.body.innerHTML = '';
});

// Music and the soundboard moved to the "In the room" block in the left panel (RoomTools).
it('the dock has no soundboard button even when the server announces the feature', () => {
  expect(document.querySelector(`button[aria-label="${t('Soundboard')}"]`)).toBeNull();
});

it('the dock has no music button even when the server offers music', () => {
  expect(document.querySelector(`button[aria-label="${t('Music')}"]`)).toBeNull();
});
